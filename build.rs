fn main() {
    // Only run this build script when compiling for Windows
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        
        // Define details visible in File Explorer Properties -> Details
        res.set("FileDescription", "XML Song Extractor Utility");
        res.set("ProductName", "XML Song Extractor");
        res.set("ProductVersion", "1.0.0.0");
        res.set("FileVersion", "1.0.0.0");
        // res.set("CompanyName", "NekoHr / Organization");
        res.set("LegalCopyright", "Copyright © 2026 NekoHr");
        res.set("OriginalFilename", "xml_song_extractor.exe");

        // (Optional) Set an app icon if you have an .ico file in your project folder
        res.set_icon("app_icon.ico");

        // Compile the resources into the final binary
        if let Err(e) = res.compile() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}