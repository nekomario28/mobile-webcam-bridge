use mobile_webcam::{control, usb};
fn main() {
    let result = (|| -> mobile_webcam::Result<()> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("--list") => {
                control::write(&mut std::io::stdout().lock(), &usb::list()?).map_err(Into::into)
            }
            Some("--switch") => {
                let id = control::read::<usb::DeviceId>(&mut std::io::stdin().lock())?
                    .ok_or("missing selected device")?;
                let parent = if args.len() == 3 && args[1] == "--parent" {
                    Some(args[2].parse::<u32>()?)
                } else {
                    None
                };
                #[cfg(target_os = "linux")]
                if unsafe { libc::geteuid() } == 0 && parent.is_none() {
                    return Err("privileged USB switching requires an application parent".into());
                }
                if let Some(parent) = parent {
                    mobile_webcam::platform::bind_parent(parent)?;
                }
                usb::switch_selected(&id, parent)
            }
            _ => Err(
                "usage: mobile-webcam-usb --list | --switch (selected device JSON on stdin)".into(),
            ),
        }
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
