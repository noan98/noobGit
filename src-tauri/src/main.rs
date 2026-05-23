// Windowsのリリースビルドでコンソール窓を出さない。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    noobgit_lib::run()
}
