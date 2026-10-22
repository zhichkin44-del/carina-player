//! Silent player self-check on real music: `cargo run --release --example check -- <folder>...`
fn main() {
    let mut total = (0, 0);
    for dir in std::env::args().skip(1) {
        let (files, failed) = carina_lib::check::folder(std::path::Path::new(&dir));
        total = (total.0 + files, total.1 + failed);
    }
    println!("\n{} files, {} with problems", total.0, total.1);
}
