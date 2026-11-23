fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint failed to compile");
    // Gives the executable its icon and version in Explorer and the taskbar.
    winresource::WindowsResource::new()
        .set_icon("../logo/clawd.ico")
        .set("ProductName", "Claude RPC")
        .set("FileDescription", "Claude RPC")
        .compile()
        .expect("the executable icon could not be embedded");
}
