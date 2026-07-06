fn main() {
    println!("cargo:rerun-if-env-changed=KINETIX_BUILD");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../assets/logo/logo.ico");
        res.compile().expect("Failed to attach icon to executable");
    }
}
