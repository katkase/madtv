//! Uscita audio verso la scheda del PC (crate `cpal`: WASAPI su Windows).
//!
//! L'emulatore produce campioni stereo (in `Paula::out`) frame per frame; qui li
//! accodiamo in un buffer condiviso con il callback della scheda audio, che li
//! preleva al proprio ritmo. La finestra usa la quantità di audio in coda per
//! decidere quando emulare il frame successivo: è l'audio a dettare il tempo,
//! così non ci sono né buchi né ritardi che crescono.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct AudioOut {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    /// Frequenza di campionamento del dispositivo.
    pub rate: u32,
    /// Nome del dispositivo, per il log.
    pub device_name: String,
}

impl AudioOut {
    /// Apre il dispositivo audio predefinito. Restituisce None se non ce n'è uno.
    pub fn open() -> Option<AudioOut> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let device_name = device
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| "?".into());
        let supported = device.default_output_config().ok()?;
        let rate = supported.sample_rate();
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let queue = Arc::new(Mutex::new(VecDeque::with_capacity(rate as usize)));
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, queue.clone()),
            SampleFormat::I16 => build::<i16>(&device, config, queue.clone()),
            SampleFormat::U16 => build::<u16>(&device, config, queue.clone()),
            SampleFormat::I32 => build::<i32>(&device, config, queue.clone()),
            SampleFormat::F64 => build::<f64>(&device, config, queue.clone()),
            _ => None,
        }?;
        stream.play().ok()?;
        Some(AudioOut {
            _stream: stream,
            queue,
            rate,
            device_name,
        })
    }

    /// Accoda campioni stereo interlacciati.
    pub fn push(&self, samples: &[f32]) {
        if let Ok(mut q) = self.queue.lock() {
            q.extend(samples.iter().copied());
        }
    }

    /// Frame stereo ancora in coda.
    pub fn queued_frames(&self) -> usize {
        self.queue.lock().map(|q| q.len() / 2).unwrap_or(0)
    }

    /// Svuota la coda (dopo un avanti veloce, per non accumulare ritardo).
    pub fn clear(&self) {
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
) -> Option<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut q = queue.lock().ok();
                for frame in data.chunks_mut(channels) {
                    let (l, r) = match q.as_mut() {
                        Some(q) if q.len() >= 2 => {
                            (q.pop_front().unwrap_or(0.0), q.pop_front().unwrap_or(0.0))
                        }
                        _ => (0.0, 0.0), // coda vuota: silenzio
                    };
                    match frame.len() {
                        1 => frame[0] = T::from_sample((l + r) * 0.5),
                        _ => {
                            frame[0] = T::from_sample(l);
                            frame[1] = T::from_sample(r);
                            for x in frame.iter_mut().skip(2) {
                                *x = T::from_sample(0.0);
                            }
                        }
                    }
                }
            },
            |e| eprintln!("errore audio: {e}"),
            None,
        )
        .ok()
}
