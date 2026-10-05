fn main() {
    println!("cargo:rustc-check-cfg=cfg(ffmpeg_9)");
    if std::env::var("DEP_FFMPEG_FFMPEG_9_0").as_deref() == Ok("true") {
        println!("cargo:rustc-cfg=ffmpeg_9");
    }
    #[cfg(feature = "video")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rerun-if-changed=build.rs");
        let bindings=bindgen::Builder::default()
            .header_contents("v4l2.h", "#include <linux/videodev2.h>\nconst unsigned long AMB_QUERYCAP=VIDIOC_QUERYCAP;\nconst unsigned long AMB_S_FMT=VIDIOC_S_FMT;\nconst unsigned long AMB_S_PARM=VIDIOC_S_PARM;\nconst unsigned int AMB_YUYV=V4L2_PIX_FMT_YUYV;\n")
            .allowlist_type("v4l2_(capability|format|streamparm)")
            .allowlist_type("v4l2_(buf_type|field|colorspace|ycbcr_encoding|quantization|xfer_func)")
            .allowlist_var("AMB_.*|V4L2_(CAP_.*|BUF_TYPE_VIDEO_OUTPUT|PIX_FMT_YUYV|FIELD_NONE|COLORSPACE_REC709|YCBCR_ENC_709|QUANTIZATION_LIM_RANGE|XFER_FUNC_709)")
            .prepend_enum_name(false).derive_default(true).generate().expect("Linux V4L2 headers are required");
        bindings
            .write_to_file(
                std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("v4l2.rs"),
            )
            .unwrap();
    }
}
