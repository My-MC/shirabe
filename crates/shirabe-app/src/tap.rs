//! Capture of what macOS plays: a Core Audio process tap of all system audio.
//!
//! Process taps exist since macOS 14.2. The tap is wrapped in a private aggregate device
//! (the documented way to read a tap) and read with an IO proc. The tap functions are looked
//! up at run time, so the application still starts on older systems and only lacks this
//! source. macOS asks the user for permission to record system audio on first use; without
//! it the tap delivers silence.

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::{null, NonNull};
use std::sync::Arc;

use anyhow::{bail, Result};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{AnyThread, Message};
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceIsStackedKey,
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceTapAutoStartKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceUID,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioSubDeviceUIDKey,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey, kAudioTapPropertyFormat,
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice,
    AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress, CATapDescription,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};

use crate::audio::{format_rate_channels, Capture, Feed};

type CreateTap = unsafe extern "C-unwind" fn(*const c_void, *mut AudioObjectID) -> i32;
type DestroyTap = unsafe extern "C-unwind" fn(AudioObjectID) -> i32;

/// `kAudioFormatFlagIsFloat`.
const FLAG_FLOAT: u32 = 1;

fn symbols() -> Option<(CreateTap, DestroyTap)> {
    // SAFETY: dlsym with RTLD_DEFAULT only looks symbols up; the types match the C
    // declarations of AudioHardwareCreateProcessTap / AudioHardwareDestroyProcessTap.
    unsafe {
        let create = libc::dlsym(
            libc::RTLD_DEFAULT,
            c"AudioHardwareCreateProcessTap".as_ptr(),
        );
        let destroy = libc::dlsym(
            libc::RTLD_DEFAULT,
            c"AudioHardwareDestroyProcessTap".as_ptr(),
        );
        if create.is_null() || destroy.is_null() {
            return None;
        }
        Some((
            std::mem::transmute::<*mut c_void, CreateTap>(create),
            std::mem::transmute::<*mut c_void, DestroyTap>(destroy),
        ))
    }
}

/// Whether this system supports process taps (macOS 14.2 and later).
pub fn available() -> bool {
    symbols().is_some() && AnyClass::get(c"CATapDescription").is_some()
}

fn address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property.
///
/// # Safety
/// `T` must be the type of the property.
unsafe fn property<T: Copy>(object: AudioObjectID, selector: u32) -> Result<T> {
    let addr = address(selector);
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = MaybeUninit::<T>::zeroed();
    let status = AudioObjectGetPropertyData(
        object,
        NonNull::from(&addr),
        0,
        null(),
        NonNull::from(&mut size),
        NonNull::new_unchecked(value.as_mut_ptr().cast()),
    );
    if status != 0 {
        bail!("Core Audio property {selector:#x} failed ({status})");
    }
    Ok(value.assume_init())
}

fn key(k: &std::ffi::CStr) -> Retained<NSString> {
    NSString::from_str(k.to_str().unwrap_or_default())
}

fn any<T: Message>(x: &T) -> &AnyObject {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { &*(x as *const T).cast::<AnyObject>() }
}

fn dictionary(
    pairs: &[(Retained<NSString>, &AnyObject)],
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = pairs.iter().map(|(k, _)| &**k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&keys, &values)
}

/// State shared with the IO proc.
struct Context {
    feed: Arc<Feed>,
}

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    data: *mut c_void,
) -> i32 {
    let ctx = &*(data as *const Context);
    let list = input.as_ptr();
    let count = (*list).mNumberBuffers as usize;
    if count == 0 {
        return 0;
    }
    let buffers: &[AudioBuffer] =
        std::slice::from_raw_parts(std::ptr::addr_of!((*list).mBuffers).cast(), count);
    let samples = |b: &AudioBuffer| -> Option<&[f32]> {
        (!b.mData.is_null()).then(|| {
            std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4)
        })
    };
    let Some(first) = samples(&buffers[0]) else {
        return 0;
    };
    if count == 1 {
        let stride = buffers[0].mNumberChannels.max(1) as usize;
        ctx.feed.with(|pump| pump.interleaved(first, stride));
    } else {
        let second = samples(&buffers[1]).filter(|s| s.len() == first.len());
        ctx.feed.with(|pump| pump.planar(first, second));
    }
    0
}

/// A process tap of all system audio.
pub struct Tap {
    destroy: DestroyTap,
    tap: AudioObjectID,
    rate: f32,
    channels: usize,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    context: *mut Context,
    _tap_uid: Retained<NSString>,
}

impl Tap {
    /// Creates the tap.
    pub fn new() -> Result<Self> {
        let Some((create, destroy)) = symbols() else {
            bail!("recording system audio needs macOS 14.2 or later");
        };
        if AnyClass::get(c"CATapDescription").is_none() {
            bail!("recording system audio needs macOS 14.2 or later");
        }
        // SAFETY: documented Core Audio calls with valid arguments.
        unsafe {
            let description = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &NSArray::new(),
            );
            description.setPrivate(true);
            let tap_uid = description.UUID().UUIDString();
            let mut tap: AudioObjectID = 0;
            let status = create(Retained::as_ptr(&description).cast(), &mut tap);
            if status != 0 || tap == 0 {
                bail!("cannot create a system audio tap ({status})");
            }
            let mut this = Self {
                destroy,
                tap,
                rate: 0.0,
                channels: 0,
                aggregate: 0,
                proc_id: None,
                context: std::ptr::null_mut(),
                _tap_uid: tap_uid,
            };
            let format: AudioStreamBasicDescription = property(tap, kAudioTapPropertyFormat)?;
            if format.mFormatFlags & FLAG_FLOAT == 0 || format.mBitsPerChannel != 32 {
                bail!(
                    "unexpected tap format ({} bit, flags {:#x})",
                    format.mBitsPerChannel,
                    format.mFormatFlags
                );
            }
            this.rate = format.mSampleRate as f32;
            this.channels = format.mChannelsPerFrame.max(1) as usize;
            Ok(this)
        }
    }

    /// Sample rate and channel count of the tap.
    pub fn format(&self) -> (f32, usize) {
        (self.rate, self.channels)
    }

    /// Starts reading the tap into `feed`.
    pub fn start(mut self, feed: Arc<Feed>) -> Result<Capture> {
        // SAFETY: documented Core Audio calls; the context outlives the IO proc (it is freed
        // in Drop after the proc is stopped and destroyed).
        unsafe {
            let output: AudioObjectID = property(
                kAudioObjectSystemObject as AudioObjectID,
                kAudioHardwarePropertyDefaultOutputDevice,
            )?;
            let uid: *mut NSString =
                property::<*mut NSString>(output, kAudioDevicePropertyDeviceUID)?;
            let Some(output_uid) = Retained::from_raw(uid) else {
                bail!("the default output device has no UID");
            };
            let yes = NSNumber::new_bool(true);
            let no = NSNumber::new_bool(false);
            let sub_device = dictionary(&[(key(kAudioSubDeviceUIDKey), any(&*output_uid))]);
            let sub_tap = dictionary(&[
                (key(kAudioSubTapDriftCompensationKey), any(&*yes)),
                (key(kAudioSubTapUIDKey), any(&*self._tap_uid)),
            ]);
            let sub_devices = NSArray::from_slice(&[&*sub_device]);
            let sub_taps = NSArray::from_slice(&[&*sub_tap]);
            let name = NSString::from_str("Shirabe system audio");
            let aggregate_uid = NSUUID::new().UUIDString();
            let description = dictionary(&[
                (key(kAudioAggregateDeviceNameKey), any(&*name)),
                (key(kAudioAggregateDeviceUIDKey), any(&*aggregate_uid)),
                (
                    key(kAudioAggregateDeviceMainSubDeviceKey),
                    any(&*output_uid),
                ),
                (key(kAudioAggregateDeviceIsPrivateKey), any(&*yes)),
                (key(kAudioAggregateDeviceIsStackedKey), any(&*no)),
                (key(kAudioAggregateDeviceTapAutoStartKey), any(&*yes)),
                (
                    key(kAudioAggregateDeviceSubDeviceListKey),
                    any(&*sub_devices),
                ),
                (key(kAudioAggregateDeviceTapListKey), any(&*sub_taps)),
            ]);
            // NSDictionary is toll-free bridged to CFDictionary.
            let cf = &*Retained::as_ptr(&description).cast::<CFDictionary>();
            let status = AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut self.aggregate));
            if status != 0 || self.aggregate == 0 {
                bail!("cannot create the capture device ({status})");
            }
            self.context = Box::into_raw(Box::new(Context { feed }));
            let status = AudioDeviceCreateIOProcID(
                self.aggregate,
                Some(io_proc),
                self.context.cast(),
                NonNull::from(&mut self.proc_id),
            );
            if status != 0 {
                bail!("cannot read the capture device ({status})");
            }
            let status = AudioDeviceStart(self.aggregate, self.proc_id);
            if status != 0 {
                bail!("cannot start the capture device ({status})");
            }
        }
        let description = format!(
            "Playing (all system audio)  ·  {}  ·  f32  ·  Core Audio tap",
            format_rate_channels(self.rate, self.channels)
        );
        Ok(Capture {
            keep: Box::new(self),
            thread: None,
            description,
        })
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: tears down what `new` and `start` created, in reverse order.
        unsafe {
            if self.aggregate != 0 {
                if self.proc_id.is_some() {
                    AudioDeviceStop(self.aggregate, self.proc_id);
                    AudioDeviceDestroyIOProcID(self.aggregate, self.proc_id);
                }
                AudioHardwareDestroyAggregateDevice(self.aggregate);
            }
            if !self.context.is_null() {
                drop(Box::from_raw(self.context));
            }
            (self.destroy)(self.tap);
        }
    }
}
