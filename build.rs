fn main() {
    // application icon (resource id 1 is what the window and taskbar use)
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/streamtube.rc");
        println!("cargo:rerun-if-changed=assets/streamtube.ico");
        println!("cargo:rerun-if-changed=assets/youtube.png");
        let _ = embed_resource::compile("assets/streamtube.rc", embed_resource::NONE);
    }
}
