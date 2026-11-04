fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Embed the application icon and version info in the Windows executable.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../../assets/icons/aurafx.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/aurafx.ico");
        res.set("ProductName", "auraFX");
        res.set("FileDescription", "auraFX scientific visualization");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
