#![feature(test)]

extern crate test;
use nanomite::*; // or just a dummy

#[bench]
fn bench_nothing(b: &mut test::Bencher) {
    b.iter(|| {});
}
