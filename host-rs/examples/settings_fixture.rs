fn main() -> mobile_webcam::Result<()> {
    {
        #[cfg(unix)]
        let path = std::env::args_os()
            .nth(1)
            .ok_or("provide an isolated INI fixture")?;
        #[cfg(unix)]
        let store = mobile_webcam::settings::Store::at(path.into());
        #[cfg(windows)]
        let store = mobile_webcam::settings::Store::native()?;
        let mut settings = store.load()?;
        assert_eq!(settings.lan_host, "192.168.4.42");
        assert_eq!(settings.rotation, 90);
        assert!(!settings.mirror);
        assert!(settings.vertical_flip);
        settings.lan_host = "192.168.4.43".into();
        settings.rotation = 270;
        for _ in 0..100 {
            store.save(&settings)?;
        }
    }
    Ok(())
}
