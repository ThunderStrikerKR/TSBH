//! Native Windows portable package acceptance: use the actual egui adapter.
#![cfg(windows)]
#[path = "../src/private_storage.rs"]
mod private_storage;
#[path = "../src/clipboard_intake.rs"]
mod clipboard_intake;

use std::{path::PathBuf,sync::atomic::AtomicBool};
use clipboard_intake::{ClipboardImage,recognize_image};

#[test]
fn cancelled_ocr_never_requires_runtime_or_starts_a_process(){
    let input=ClipboardImage{width:1,height:1,rgba:vec![255;4]};
    assert_eq!(recognize_image(&input,&AtomicBool::new(true)).unwrap_err(),"OCR_CANCELLED");
}

#[test]
#[ignore = "requires extracted portable package and synthetic image"]
fn relocated_portable_ocr_recognizes_synthetic_image(){
    assert!(std::env::var_os("BANDO_OCR_RUNTIME_DIR").is_none());
    let exe=std::env::current_exe().unwrap();
    let root=exe.parent().unwrap();
    assert!(root.join("bando-hub-egui.exe").is_file());
    assert!(root.join("ocr/runtime/python.exe").is_file());
    for model in ["PP-OCRv5_mobile_det","korean_PP-OCRv5_mobile_rec"]{
        for file in ["inference.json","inference.pdiparams","inference.yml"]{
            assert!(root.join("ocr/models").join(model).join(file).is_file());
        }
    }
    let path=PathBuf::from(std::env::var_os("BANDO_CI_OCR_SYNTHETIC_IMAGE")
        .expect("explicit synthetic image path required"));
    // PNG decoding is enabled in the egui image crate; the native adapter
    // itself converts these RGBA pixels to its production BMP input.
    let pixels=image::open(&path).expect("read synthetic PNG").into_rgba8();
    let input=ClipboardImage{width:pixels.width() as usize,height:pixels.height() as usize,
        rgba:pixels.into_raw()};
    let text=recognize_image(&input,&AtomicBool::new(false)).expect("native packaged OCR");
    let compact:String=text.chars().filter(|c|!c.is_whitespace()).collect();
    assert!(compact.contains("709"),"missing expected synthetic product code: {text}");
    assert!(compact.contains("반도"),"missing expected Korean text: {text}");
}
