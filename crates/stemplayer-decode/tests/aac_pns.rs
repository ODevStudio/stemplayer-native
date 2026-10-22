use symphonia::core::audio::{Channels, Position};
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::codecs::audio::well_known::profiles::CODEC_PROFILE_AAC_LC;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Timestamp};

fn stereo_noise_codebook_packet(index: i64) -> Packet {
    Packet::new(
        1,
        Timestamp::new(index * 1024),
        Duration::new(1024),
        vec![
            0x20, 0xa0, 0x00, 0x5a, 0x18, 0x00, 0xa0, 0x00, 0x5a, 0x18, 0x00, 0xe0,
        ],
    )
}

fn decoder() -> Box<dyn AudioDecoder> {
    let mut parameters = AudioCodecParameters::new();
    parameters.codec = CODEC_ID_AAC;
    parameters.profile = Some(CODEC_PROFILE_AAC_LC);
    parameters.sample_rate = Some(44_100);
    parameters.channels = Some(Channels::Positioned(
        Position::FRONT_LEFT | Position::FRONT_RIGHT,
    ));
    parameters.extra_data = Some(vec![0x12, 0x10].into_boxed_slice());
    symphonia::default::get_codecs()
        .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
        .unwrap()
}

fn decode_noise_window(decoder: &mut dyn AudioDecoder, first_packet: i64) -> Vec<f32> {
    (first_packet..first_packet + 3).fold(Vec::new(), |_, index| {
        let packet = stereo_noise_codebook_packet(index);
        let decoded = decoder.decode(&packet).unwrap();
        assert_eq!(decoded.frames(), 1024);
        assert_eq!(decoded.spec().rate(), 44_100);
        assert_eq!(decoded.spec().channels().count(), 2);
        let mut samples = vec![0.0_f32; 2048];
        decoded.copy_to_slice_interleaved(&mut samples);
        assert!(samples.iter().all(|sample| sample.is_finite()));
        samples
    })
}

#[test]
fn original_noise_codebook_packet_decodes_to_finite_nonzero_stereo() {
    let samples = decode_noise_window(decoder().as_mut(), 0);
    assert!(samples.iter().step_by(2).any(|sample| *sample != 0.0));
    assert!(
        samples
            .iter()
            .skip(1)
            .step_by(2)
            .any(|sample| *sample != 0.0)
    );
}

#[test]
#[ignore = "blocked on qualified AAC PNS decoder state restoration"]
fn repeated_aac_pns_seek_restores_identical_pcm() {
    let mut decoder = decoder();
    let first = decode_noise_window(decoder.as_mut(), 5);
    decoder.reset();
    let repeated = decode_noise_window(decoder.as_mut(), 5);
    assert_eq!(first, repeated);
}

#[test]
#[ignore = "blocked on qualified AAC PNS decoder state restoration"]
fn aac_pns_seek_with_two_packet_preroll_matches_sequential_pcm() {
    let mut sequential = decoder();
    for index in 0..5 {
        sequential
            .decode(&stereo_noise_codebook_packet(index))
            .unwrap();
    }
    let expected = decode_noise_window(sequential.as_mut(), 5);
    let actual = decode_noise_window(decoder().as_mut(), 5);
    assert_eq!(expected, actual);
}
