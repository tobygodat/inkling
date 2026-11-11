fn main() {
    inkling_app::configure(inkling_core::Paths::mac());
    rune::run(inkling_app::spec());
}
