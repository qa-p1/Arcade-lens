//! Prints the cheap scheduling signals for an image: `cargo run --example signals -- shot.png`
fn main() {
    let p = std::env::args().nth(1).expect("image path");
    let img = image::open(p).expect("readable image").to_rgba8();
    let s = lens_core::Signals::compute(&img);
    println!("{s:?}\nuniform: {}", s.is_uniform());
}
