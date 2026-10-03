//! Writes a one-page PDF from an image: `cargo run --example pdf -- in.png out.pdf`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let img = image::open(&a[1]).expect("readable image").to_rgba8();
    std::fs::write(&a[2], lens_actions::extra::pdf_from_image(&img).expect("pdf")).expect("write");
}
