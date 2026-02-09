use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::core::audio::RecordedAudio;

pub struct ImportedAudio {
    pub audio: RecordedAudio,
    pub duration_ms: u32,
}

pub fn decode_audio_file(path: &Path) -> Result<ImportedAudio, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|err| err.to_string())?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "No supported audio tracks found".to_string())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|err| err.to_string())?;

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = codec_params.sample_rate.unwrap_or(0);
    let mut channels = codec_params
        .channels
        .map(|channels| channels.count())
        .unwrap_or(0);

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => {
                return Err("Decoder reset required while reading audio".to_string());
            }
            Err(err) => return Err(err.to_string()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(Error::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::DecodeError(_)) => continue,
            Err(err) => return Err(err.to_string()),
        };

        let buf = decoded;
        sample_rate = buf.spec().rate;
        channels = buf.spec().channels.count();
        let mut sample_buf = SampleBuffer::<f32>::new(buf.capacity() as u64, *buf.spec());
        sample_buf.copy_interleaved_ref(buf);
        samples.extend_from_slice(sample_buf.samples());
    }

    if samples.is_empty() {
        return Err("No audio samples decoded".to_string());
    }
    if sample_rate == 0 || channels == 0 {
        return Err("Audio stream metadata missing sample rate or channels".to_string());
    }

    let frames = samples.len() / channels.max(1) as usize;
    let duration_ms = ((frames as u64).saturating_mul(1000) / sample_rate as u64) as u32;

    Ok(ImportedAudio {
        audio: RecordedAudio {
            samples,
            sample_rate,
            channels: channels as u16,
        },
        duration_ms,
    })
}
