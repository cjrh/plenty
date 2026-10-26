//! Fallible, borrowed-buffer linear algebra engine for Plenty.
//!
//! No native owner crosses this boundary. The trusted facade supplies disjoint,
//! initialized strided lists; every view lives for one call. Only the low-level
//! sequential faer kernels are used, with fallibly allocated scratch storage.
#![allow(clippy::too_many_arguments)]

use dyn_stack::{MemBuffer, MemStack, StackReq};
use faer::{Accum, Conj, MatMut, MatRef, Par, linalg};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[cfg(test)]
mod tests;

const ALLOC: i64 = 1;
const SHAPE: i64 = 2;
const SINGULAR: i64 = 3;
const NOT_POSITIVE: i64 = 4;
const NO_CONVERGENCE: i64 = 5;
const OVERFLOW: i64 = 6;
const PANIC: i64 = 7;
type Result<T> = std::result::Result<T, i64>;

fn boundary(f: impl FnOnce() -> Result<()>) -> i64 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(code)) => code,
        Err(_) => PANIC,
    }
}

fn dims(m: i64, n: i64) -> Result<(usize, usize)> {
    if m <= 0 || n <= 0 {
        return Err(SHAPE);
    }
    let (m, n) = (m as usize, n as usize);
    m.checked_mul(n)
        .filter(|&len| len <= isize::MAX as usize / 16)
        .ok_or(SHAPE)?;
    Ok((m, n))
}

fn view_check(data: *const u8, len: usize, stride: usize, m: usize, n: usize) -> Result<()> {
    if data.is_null()
        || !(data as usize).is_multiple_of(align_of::<f64>())
        || stride < size_of::<f64>()
        || !stride.is_multiple_of(size_of::<f64>())
        || len != m.checked_mul(n).ok_or(SHAPE)?
        || len
            .checked_mul(stride)
            .is_none_or(|bytes| bytes > isize::MAX as usize)
    {
        return Err(SHAPE);
    }
    Ok(())
}

// SAFETY: callers must lend valid, aligned storage for every logical element.
unsafe fn input<'a>(
    p: *const u8,
    len: usize,
    stride: usize,
    m: usize,
    n: usize,
) -> Result<MatRef<'a, f64>> {
    view_check(p, len, stride, m, n)?;
    let step = (stride / size_of::<f64>()) as isize;
    // SAFETY: checked byte span/shape, caller guarantees the allocation lifetime.
    let a: MatRef<'_, f64> =
        unsafe { MatRef::from_raw_parts(p.cast(), m, n, step, step * m as isize) };
    for col in 0..n {
        for row in 0..m {
            if !a[(row, col)].is_finite() {
                return Err(OVERFLOW);
            }
        }
    }
    Ok(a)
}

// SAFETY: storage must be exclusive and disjoint from every other view in use.
unsafe fn output<'a>(
    p: *mut u8,
    len: usize,
    stride: usize,
    m: usize,
    n: usize,
) -> Result<MatMut<'a, f64>> {
    view_check(p, len, stride, m, n)?;
    let step = (stride / size_of::<f64>()) as isize;
    // SAFETY: checked byte span/shape, caller guarantees exclusive live storage.
    Ok(unsafe { MatMut::from_raw_parts_mut(p.cast(), m, n, step, step * m as isize) })
}

fn finite(a: MatRef<'_, f64>) -> Result<()> {
    for col in 0..a.ncols() {
        for row in 0..a.nrows() {
            if !a[(row, col)].is_finite() {
                return Err(OVERFLOW);
            }
        }
    }
    Ok(())
}

fn scratch(req: StackReq) -> Result<MemBuffer> {
    MemBuffer::try_new(req).map_err(|_| ALLOC)
}

fn zeros<T: Copy + Default>(n: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(n).map_err(|_| ALLOC)?;
    values.resize(n, T::default());
    Ok(values)
}

/// # Safety
/// All list triples must describe live initialized storage; output is exclusive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_multiply(
    m: i64,
    n: i64,
    k: i64,
    a: *const u8,
    al: usize,
    ast: usize,
    b: *const u8,
    bl: usize,
    bst: usize,
    c: *mut u8,
    cl: usize,
    cst: usize,
) -> i64 {
    boundary(|| {
        let (m, n) = dims(m, n)?;
        let (_, k) = dims(m as i64, k)?;
        // SAFETY: facade borrows inputs and allocates a separate output list.
        let (a, b, mut c) = unsafe {
            (
                input(a, al, ast, m, k)?,
                input(b, bl, bst, k, n)?,
                output(c, cl, cst, m, n)?,
            )
        };
        linalg::matmul::matmul(&mut c, Accum::Replace, a, b, 1.0, Par::Seq);
        finite(c.as_ref())
    })
}

/// # Safety
/// `a` and `perm` are exclusive, disjoint initialized strided lists.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_lu(
    m: i64,
    n: i64,
    a: *mut u8,
    al: usize,
    ast: usize,
    perm: *mut u8,
    pl: usize,
    pst: usize,
) -> i64 {
    boundary(|| {
        let (m, n) = dims(m, n)?;
        view_check(perm, pl, pst, m, 1)?;
        // SAFETY: facade supplies a fresh copy of its matrix.
        let mut a = unsafe { output(a, al, ast, m, n)? };
        finite(a.as_ref())?;
        let mut p = zeros::<usize>(m)?;
        let mut inv = zeros::<usize>(m)?;
        let mut mem = scratch(linalg::lu::partial_pivoting::factor::lu_in_place_scratch::<
            usize,
            f64,
        >(m, n, Par::Seq, Default::default()))?;
        linalg::lu::partial_pivoting::factor::lu_in_place(
            a.as_mut(),
            &mut p,
            &mut inv,
            Par::Seq,
            MemStack::new(&mut mem),
            Default::default(),
        );
        finite(a.as_ref())?;
        for (i, &value) in p.iter().enumerate() {
            // SAFETY: checked permutation span; every i is within its length.
            unsafe { perm.add(i * pst).cast::<i64>().write(value as i64) };
        }
        Ok(())
    })
}

/// # Safety
/// Input lists are borrowed; rhs is exclusive, initialized and disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_solve(
    n: i64,
    k: i64,
    a: *const u8,
    al: usize,
    ast: usize,
    perm: *const u8,
    pl: usize,
    pst: usize,
    rhs: *mut u8,
    rl: usize,
    rst: usize,
) -> i64 {
    boundary(|| {
        let (n, k) = dims(n, k)?;
        view_check(perm, pl, pst, n, 1)?;
        // SAFETY: facade lends LU and a separate RHS copy.
        let (a, mut rhs) = unsafe { (input(a, al, ast, n, n)?, output(rhs, rl, rst, n, k)?) };
        for i in 0..n {
            if a[(i, i)] == 0.0 {
                return Err(SINGULAR);
            }
        }
        let mut p = zeros::<usize>(n)?;
        let mut inv = zeros::<usize>(n)?;
        inv.fill(usize::MAX);
        for (i, slot) in p.iter_mut().enumerate() {
            // SAFETY: checked permutation span, initialized i64 elements.
            let value = unsafe { perm.add(i * pst).cast::<i64>().read() };
            let value = usize::try_from(value).map_err(|_| SHAPE)?;
            if value >= n || inv[value] != usize::MAX {
                return Err(SHAPE);
            }
            *slot = value;
            inv[value] = i;
        }
        // SAFETY: p is a validated bijection and inv its exact inverse.
        let permutation = unsafe { faer::perm::PermRef::new_unchecked(&p, &inv, n) };
        let mut mem = scratch(
            linalg::lu::partial_pivoting::solve::solve_in_place_scratch::<usize, f64>(
                n,
                k,
                Par::Seq,
            ),
        )?;
        linalg::lu::partial_pivoting::solve::solve_in_place(
            a,
            a,
            permutation,
            rhs.as_mut(),
            Par::Seq,
            MemStack::new(&mut mem),
        );
        finite(rhs.as_ref())
    })
}

/// # Safety
/// Both lists are exclusive, disjoint and initialized; q starts as identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_qr(
    m: i64,
    n: i64,
    a: *mut u8,
    al: usize,
    ast: usize,
    q: *mut u8,
    ql: usize,
    qst: usize,
) -> i64 {
    boundary(|| {
        let (m, n) = dims(m, n)?;
        // SAFETY: facade allocates two disjoint matrices.
        let (mut a, mut q) = unsafe { (output(a, al, ast, m, n)?, output(q, ql, qst, m, m)?) };
        finite(a.as_ref())?;
        let rank = m.min(n);
        let block = linalg::qr::no_pivoting::factor::recommended_block_size::<f64>(m, n);
        let mut coefficients = zeros::<f64>(block.checked_mul(rank).ok_or(SHAPE)?)?;
        let mut coeff = MatMut::from_column_major_slice_mut(&mut coefficients, block, rank);
        let factor_req = linalg::qr::no_pivoting::factor::qr_in_place_scratch::<f64>(
            m,
            n,
            block,
            Par::Seq,
            Default::default(),
        );
        let apply_req =
            linalg::householder::apply_block_householder_sequence_on_the_left_in_place_scratch::<f64>(
                m, block, m,
            );
        let mut mem = scratch(StackReq::any_of(&[factor_req, apply_req]))?;
        linalg::qr::no_pivoting::factor::qr_in_place(
            a.as_mut(),
            coeff.as_mut(),
            Par::Seq,
            MemStack::new(&mut mem),
            Default::default(),
        );
        linalg::householder::apply_block_householder_sequence_on_the_left_in_place_with_conj(
            a.as_ref().get(.., ..rank),
            coeff.as_ref(),
            Conj::No,
            q.as_mut(),
            Par::Seq,
            MemStack::new(&mut mem),
        );
        finite(a.as_ref())?;
        finite(q.as_ref())
    })
}

/// # Safety
/// Matrix storage is exclusive and initialized, with n*n elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_cholesky(n: i64, a: *mut u8, al: usize, ast: usize) -> i64 {
    boundary(|| {
        let (n, _) = dims(n, n)?;
        // SAFETY: facade lends a fresh matrix copy.
        let mut a = unsafe { output(a, al, ast, n, n)? };
        finite(a.as_ref())?;
        let mut mem = scratch(linalg::cholesky::llt::factor::cholesky_in_place_scratch::<
            f64,
        >(n, Par::Seq, Default::default()))?;
        linalg::cholesky::llt::factor::cholesky_in_place(
            a.as_mut(),
            Default::default(),
            Par::Seq,
            MemStack::new(&mut mem),
            Default::default(),
        )
        .map_err(|_| NOT_POSITIVE)?;
        finite(a.as_ref())
    })
}

/// # Safety
/// Output lists are exclusive, initialized and disjoint from a and each other.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_svd(
    m: i64,
    n: i64,
    a: *const u8,
    al: usize,
    ast: usize,
    s: *mut u8,
    sl: usize,
    sst: usize,
    u: *mut u8,
    ul: usize,
    ust: usize,
    v: *mut u8,
    vl: usize,
    vst: usize,
) -> i64 {
    boundary(|| {
        let (m, n) = dims(m, n)?;
        let rank = m.min(n);
        // SAFETY: facade allocates all three outputs independently.
        let (a, mut s, mut u, mut v) = unsafe {
            (
                input(a, al, ast, m, n)?,
                output(s, sl, sst, rank, 1)?,
                output(u, ul, ust, m, rank)?,
                output(v, vl, vst, n, rank)?,
            )
        };
        let mut mem = scratch(linalg::svd::svd_scratch::<f64>(
            m,
            n,
            linalg::svd::ComputeSvdVectors::Thin,
            linalg::svd::ComputeSvdVectors::Thin,
            Par::Seq,
            Default::default(),
        ))?;
        linalg::svd::svd(
            a,
            s.as_mut().col_mut(0).as_diagonal_mut(),
            Some(u.as_mut()),
            Some(v.as_mut()),
            Par::Seq,
            MemStack::new(&mut mem),
            Default::default(),
        )
        .map_err(|_| NO_CONVERGENCE)?;
        finite(s.as_ref())?;
        finite(u.as_ref())?;
        finite(v.as_ref())
    })
}

/// # Safety
/// Outputs are exclusive, initialized and disjoint from the input and each other.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_eigh(
    n: i64,
    a: *const u8,
    al: usize,
    ast: usize,
    s: *mut u8,
    sl: usize,
    sst: usize,
    u: *mut u8,
    ul: usize,
    ust: usize,
) -> i64 {
    boundary(|| {
        let (n, _) = dims(n, n)?;
        // SAFETY: facade allocates separate eigenvalue/eigenvector lists.
        let (a, mut s, mut u) = unsafe {
            (
                input(a, al, ast, n, n)?,
                output(s, sl, sst, n, 1)?,
                output(u, ul, ust, n, n)?,
            )
        };
        let mut mem = scratch(linalg::evd::self_adjoint_evd_scratch::<f64>(
            n,
            linalg::evd::ComputeEigenvectors::Yes,
            Par::Seq,
            Default::default(),
        ))?;
        linalg::evd::self_adjoint_evd(
            a,
            s.as_mut().col_mut(0).as_diagonal_mut(),
            Some(u.as_mut()),
            Par::Seq,
            MemStack::new(&mut mem),
            Default::default(),
        )
        .map_err(|_| NO_CONVERGENCE)?;
        finite(s.as_ref())?;
        finite(u.as_ref())
    })
}

/// # Safety
/// Every output is initialized, exclusive, and disjoint from the input and
/// other outputs. Real/imaginary parts use separate Plenty-owned buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plla_v1_eig(
    n: i64,
    a: *const u8,
    al: usize,
    ast: usize,
    real: *mut u8,
    rl: usize,
    rst: usize,
    imag: *mut u8,
    il: usize,
    ist: usize,
    vr: *mut u8,
    vrl: usize,
    vrst: usize,
    vi: *mut u8,
    vil: usize,
    vist: usize,
) -> i64 {
    boundary(|| {
        let (n, _) = dims(n, n)?;
        // SAFETY: all lists are independently allocated by the trusted facade.
        let (a, mut real, mut imag, mut vr, mut vi) = unsafe {
            (
                input(a, al, ast, n, n)?,
                output(real, rl, rst, n, 1)?,
                output(imag, il, ist, n, 1)?,
                output(vr, vrl, vrst, n, n)?,
                output(vi, vil, vist, n, n)?,
            )
        };
        let mut complex = zeros::<faer::c64>(n * n)?;
        for col in 0..n {
            for row in 0..n {
                complex[col * n + row] = faer::c64::new(a[(row, col)], 0.0);
            }
        }
        let mut values = zeros::<faer::c64>(n)?;
        let mut vectors = zeros::<faer::c64>(n * n)?;
        let mut mem = scratch(linalg::evd::evd_scratch::<faer::c64>(
            n,
            linalg::evd::ComputeEigenvectors::No,
            linalg::evd::ComputeEigenvectors::Yes,
            Par::Seq,
            Default::default(),
        ))?;
        linalg::evd::evd_cplx(
            MatRef::from_column_major_slice(&complex, n, n),
            MatMut::from_column_major_slice_mut(&mut values, n, 1)
                .col_mut(0)
                .as_diagonal_mut(),
            None,
            Some(MatMut::from_column_major_slice_mut(&mut vectors, n, n)),
            Par::Seq,
            MemStack::new(&mut mem),
            Default::default(),
        )
        .map_err(|_| NO_CONVERGENCE)?;
        for col in 0..n {
            real[(col, 0)] = values[col].re;
            imag[(col, 0)] = values[col].im;
            for row in 0..n {
                vr[(row, col)] = vectors[col * n + row].re;
                vi[(row, col)] = vectors[col * n + row].im;
            }
        }
        finite(real.as_ref())?;
        finite(imag.as_ref())?;
        finite(vr.as_ref())?;
        finite(vi.as_ref())
    })
}
