//! Sine and cosine as the canonical C build computes them.
//!
//! Built with `-O3`, clang pairs every `sin(x)` with the `cos(x)` of the same
//! argument into one `__sincos_stret(x)` on Darwin, and in cbirds every call
//! has such a partner (docs/evidence/trig-sites.txt). On arm64 that routine
//! differs from separate `sin` and `cos` by an ulp for about one argument in
//! eight hundred, so the Rust port calls it too, wherever the C calls either.
//! On x86_64 Darwin and in glibc the combined and separate routines agree bit
//! for bit (measured over 2×10⁷ arguments), and the separate ones are used.

#[cfg(target_vendor = "apple")]
#[repr(C)]
struct Double2 {
    sin: f64,
    cos: f64,
}

#[cfg(target_vendor = "apple")]
unsafe extern "C" {
    /// libSystem's `struct __double2 __sincos_stret(double)`, declared in
    /// <math.h>; the pair comes back in two floating-point registers on both
    /// architectures, which `#[repr(C)]` over two doubles matches.
    fn __sincos_stret(x: f64) -> Double2;
}

/// `(sin(x), cos(x))`.
#[inline]
pub fn sin_cos(x: f64) -> (f64, f64) {
    #[cfg(target_vendor = "apple")]
    {
        // SAFETY: a pure function of one double, with no pointers and no
        // global state; the declaration matches libSystem's prototype.
        let pair = unsafe { __sincos_stret(x) };
        (pair.sin, pair.cos)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        (x.sin(), x.cos())
    }
}
