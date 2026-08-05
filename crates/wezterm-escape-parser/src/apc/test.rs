use super::*;
use k9::assert_equal as assert_eq;

#[test]
fn kitty_payload() {
    assert_eq!(
        KittyImage::parse_apc("Gf=24,s=10,v=20;aGVsbG8=".as_bytes()).unwrap(),
        KittyImage::TransmitData {
            transmit: KittyImageTransmit {
                format: Some(KittyImageFormat::Rgb),
                data: KittyImageData::Direct("aGVsbG8=".to_string()),
                width: Some(10),
                height: Some(20),
                image_id: None,
                image_number: None,
                compression: KittyImageCompression::None,
                more_data_follows: false,
            },
            verbosity: KittyImageVerbosity::Verbose,
        }
    );

    assert_eq!(
        KittyImage::parse_apc("Ga=d,q=2".as_bytes()).unwrap(),
        KittyImage::Delete {
            what: KittyImageDelete::All { delete: false },
            verbosity: KittyImageVerbosity::Quiet
        }
    );

    assert_eq!(
        KittyImage::parse_apc("Ga=f,x=119,y=384,s=17,v=32,i=7257421,X=1,r=1,q=2;AAAA=".as_bytes())
            .unwrap(),
        KittyImage::TransmitFrame {
            transmit: KittyImageTransmit {
                format: None,
                data: KittyImageData::Direct("AAAA=".to_string()),
                width: Some(17),
                height: Some(32),
                image_id: Some(7257421),
                image_number: None,
                compression: KittyImageCompression::None,
                more_data_follows: false,
            },
            verbosity: KittyImageVerbosity::Quiet,
            frame: KittyImageFrame {
                x: Some(119),
                y: Some(384),
                base_frame: None,
                frame_number: Some(1),
                composition_mode: KittyFrameCompositionMode::Overwrite,
                background_pixel: None,
                duration_ms: None,
            },
        }
    );
}
