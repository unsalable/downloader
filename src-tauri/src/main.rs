// The app logs to files, never to stdout, so it has no use for a console at
// any point. Making the subsystem unconditional keeps a black console window
// from appearing behind the window in local builds as well as released ones.
#![windows_subsystem = "windows"]

fn main() {
    universal_downloader_lib::run()
}
