fn main() {
    // 前端资源（frontendDist = ui/）在编译期内嵌进二进制；不声明
    // rerun-if-changed 的话，ui/ 的修改不会触发 build.rs 重跑，
    // 导致改了页面但二进制里仍是旧资源（页面"改了没生效"）。
    println!("cargo:rerun-if-changed=ui");
    tauri_build::build()
}
