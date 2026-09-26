//! Floating-point evaluation as the canonical C reference build performs it.
//!
//! cbirds is built with `-std=c99 -O3` and no fast-math. Under that contract a
//! C compiler may still *contract* `a * b + c` inside one expression into a
//! single fused multiply-add, and whether it does is a property of the compiler
//! and target, not of the source:
//!
//! - Apple clang (the canonical Darwin control) defaults to
//!   `-ffp-contract=on`. Clang's code generator turns each `+`/`-` whose left
//!   operand, or failing that right operand, is a multiplication with no other
//!   use into `llvm.fmuladd`. On arm64 that is one `fmadd`, rounded once; on
//!   baseline x86-64, which has no FMA unit, it is lowered back to a separate
//!   multiply and add.
//! - GCC (the canonical GNU/Linux control) defaults to `-ffp-contract=off` in
//!   the ISO C modes the makefile selects, so it never fuses.
//!
//! Rust never contracts on its own, so every site where the canonical control
//! fuses is written as [`mul_add`] and tagged with the C source location that
//! produced it, as `fma: <file>:<line>:<column>`. The complete list of sites is
//! generated from the reference's debug info by `tools/oracle/fma-sites.sh`
//! and checked against those tags by the `fma_sites` test, so a site cannot be
//! silently dropped or invented.
//!
//! [`CONTRACTS`] selects fusion for exactly the target whose canonical control
//! fuses. Additional controls (Clang on Linux) are characterized separately,
//! as docs/PORTING.md §2 requires, rather than changing expected results here.

#![forbid(unsafe_code)]

/// Whether the canonical C control for this target fuses `llvm.fmuladd`.
pub const CONTRACTS: bool = cfg!(all(target_arch = "aarch64", target_vendor = "apple"));

/// `a * b + c` evaluated the way the canonical C build evaluates the tagged
/// source expression: fused and rounded once where that build fuses, otherwise
/// a rounded product followed by a rounded sum.
///
/// A C `a * b - c` is `mul_add(a, b, -c)` and `c - a * b` is
/// `mul_add(-a, b, c)`: negation is exact, so both spellings also agree with
/// the unfused evaluation bit for bit.
#[inline(always)]
pub fn mul_add(a: f64, b: f64, c: f64) -> f64 {
    if CONTRACTS { a.mul_add(b, c) } else { a * b + c }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fusion_rounds_once_only_where_the_control_fuses() {
        // 1 + 2^-30 squared is 1 + 2^-29 + 2^-60; the last term is lost to a
        // rounded product and kept, then cancelled exactly, by a fused one.
        let a = 1.0 + f64::EPSILON.sqrt() / 2.0_f64.powi(4);
        let product = a * a;
        let fused = mul_add(a, a, -product);
        if CONTRACTS {
            assert_ne!(fused, 0.0);
            assert_eq!(fused, a.mul_add(a, -product));
        } else {
            assert_eq!(fused, 0.0);
        }
    }

    #[test]
    fn subtraction_spellings_match_unfused_evaluation_when_not_contracting() {
        let (a, b, c) = (0.1_f64, 0.7_f64, 0.3_f64);
        if !CONTRACTS {
            assert_eq!(mul_add(a, b, -c).to_bits(), (a * b - c).to_bits());
            assert_eq!(mul_add(-a, b, c).to_bits(), (c - a * b).to_bits());
        }
    }
}
