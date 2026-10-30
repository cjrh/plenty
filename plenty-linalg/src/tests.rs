use super::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static BUDGET: Cell<Option<usize>> = const { Cell::new(None) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct TestAllocator;
#[global_allocator]
static ALLOCATOR: TestAllocator = TestAllocator;

fn permitted() -> bool {
    ALLOCATIONS.with(|n| n.set(n.get() + 1));
    BUDGET.with(|budget| match budget.get() {
        None => true,
        Some(0) => false,
        Some(n) => {
            budget.set(Some(n - 1));
            true
        }
    })
}

// SAFETY: successful operations delegate to System with unchanged layouts.
unsafe impl GlobalAlloc for TestAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !permitted() {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !permitted() {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if !permitted() {
            return std::ptr::null_mut();
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

fn budgeted(n: usize, run: impl FnOnce() -> i64) -> (i64, usize) {
    ALLOCATIONS.with(|a| a.set(0));
    BUDGET.with(|b| b.set(Some(n)));
    let result = run();
    BUDGET.with(|b| b.set(None));
    (result, ALLOCATIONS.with(Cell::get))
}

#[test]
fn cold_workspace_failure_then_allocation_free_reuse() {
    const CHILD: &str = "PLENTY_LINALG_COLD_WORKSPACE_TEST";
    if std::env::var_os(CHILD).is_none() {
        assert!(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::cold_workspace_failure_then_allocation_free_reuse"
                ])
                .env(CHILD, "1")
                .status()
                .unwrap()
                .success()
        );
        return;
    }
    let n = 17;
    let a = vec![1.0f64; n * n];
    let mut c = vec![0.0f64; n * n];
    let mut multiply = || unsafe {
        plla_v1_multiply(
            n as i64,
            n as i64,
            n as i64,
            a.as_ptr().cast(),
            a.len(),
            8,
            a.as_ptr().cast(),
            a.len(),
            8,
            c.as_mut_ptr().cast(),
            c.len(),
            8,
        )
    };
    let (cold, _) = budgeted(0, &mut multiply);
    #[cfg(target_arch = "x86_64")]
    let uses_packing = std::is_x86_feature_detected!("avx512f")
        || (std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma"));
    #[cfg(not(target_arch = "x86_64"))]
    let uses_packing = false;
    if uses_packing {
        assert_eq!(cold, ALLOC);
    } else {
        assert_eq!(cold, 0);
    }
    // Returning a workspace to the fixed-size pool performs no allocation.
    assert_eq!(budgeted(1, &mut multiply).0, 0);
    assert_eq!(budgeted(2, &mut multiply).0, 0);
    assert_eq!(budgeted(0, &mut multiply), (0, 0));
    assert!(c.iter().all(|&value| value == n as f64));
}

#[test]
fn concurrent_calls_have_independent_packing_buffers() {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (1..=8)
        .map(|value| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let n = 65;
                let a = vec![value as f64; n * n];
                let mut c = vec![0.0; n * n];
                barrier.wait();
                for _ in 0..8 {
                    let code = unsafe {
                        plla_v1_multiply(
                            n as i64,
                            n as i64,
                            n as i64,
                            a.as_ptr().cast(),
                            a.len(),
                            8,
                            a.as_ptr().cast(),
                            a.len(),
                            8,
                            c.as_mut_ptr().cast(),
                            c.len(),
                            8,
                        )
                    };
                    assert_eq!(code, 0);
                    assert!(c.iter().all(|&x| x == (value * value * n) as f64));
                }
            })
        })
        .collect();
    for thread in handles {
        thread.join().unwrap();
    }
}

#[test]
fn multiply_has_only_the_fallible_workspace_allocation() {
    for n in [1, 17, 64, 129] {
        let a = vec![1.0; n * n];
        let mut out = vec![0.0; n * n];
        let (code, allocations) = budgeted(1, || unsafe {
            plla_v1_multiply(
                n as i64,
                n as i64,
                n as i64,
                a.as_ptr().cast(),
                a.len(),
                8,
                a.as_ptr().cast(),
                a.len(),
                8,
                out.as_mut_ptr().cast(),
                out.len(),
                8,
            )
        });
        assert_eq!(code, 0);
        assert!(allocations <= 1);
        assert!(out.iter().all(|&value| value == n as f64));
    }
}

#[test]
fn every_native_allocation_is_recoverable() {
    // Larger sizes exercise the blocked LU/QR and divide-and-conquer SVD/EVD
    // paths; global denial aborts the test process if any hidden infallible
    // allocation is reintroduced, including within a dependency.
    for n in [1, 17, 65, 129] {
        let mut a = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                a[j * n + i] = if i == j { n as f64 + 1.0 } else { 0.25 };
            }
        }
        for operation in 0..7 {
            let mut succeeded = false;
            for budget in 0..10 {
                let mut packed = a.clone();
                let mut p: Vec<i64> = (0..n as i64).collect();
                let mut s = vec![0.0; n];
                let mut si = vec![0.0; n];
                let mut u = a.clone();
                let mut v = a.clone();
                let (code, _) = budgeted(budget, || unsafe {
                    match operation {
                        0 => plla_v1_lu(
                            n as i64,
                            n as i64,
                            packed.as_mut_ptr().cast(),
                            packed.len(),
                            8,
                            p.as_mut_ptr().cast(),
                            p.len(),
                            8,
                        ),
                        1 => plla_v1_solve(
                            n as i64,
                            n as i64,
                            a.as_ptr().cast(),
                            a.len(),
                            8,
                            p.as_ptr().cast(),
                            p.len(),
                            8,
                            u.as_mut_ptr().cast(),
                            u.len(),
                            8,
                        ),
                        2 => plla_v1_qr(
                            n as i64,
                            n as i64,
                            packed.as_mut_ptr().cast(),
                            packed.len(),
                            8,
                            u.as_mut_ptr().cast(),
                            u.len(),
                            8,
                        ),
                        3 => {
                            plla_v1_cholesky(n as i64, packed.as_mut_ptr().cast(), packed.len(), 8)
                        }
                        4 => plla_v1_svd(
                            n as i64,
                            n as i64,
                            a.as_ptr().cast(),
                            a.len(),
                            8,
                            s.as_mut_ptr().cast(),
                            s.len(),
                            8,
                            u.as_mut_ptr().cast(),
                            u.len(),
                            8,
                            v.as_mut_ptr().cast(),
                            v.len(),
                            8,
                        ),
                        5 => plla_v1_eigh(
                            n as i64,
                            a.as_ptr().cast(),
                            a.len(),
                            8,
                            s.as_mut_ptr().cast(),
                            s.len(),
                            8,
                            u.as_mut_ptr().cast(),
                            u.len(),
                            8,
                        ),
                        _ => plla_v1_eig(
                            n as i64,
                            a.as_ptr().cast(),
                            a.len(),
                            8,
                            s.as_mut_ptr().cast(),
                            s.len(),
                            8,
                            si.as_mut_ptr().cast(),
                            si.len(),
                            8,
                            u.as_mut_ptr().cast(),
                            u.len(),
                            8,
                            v.as_mut_ptr().cast(),
                            v.len(),
                            8,
                        ),
                    }
                });
                assert!(
                    code == ALLOC || code == 0,
                    "operation={operation} n={n} budget={budget}: {code}"
                );
                if code == 0 {
                    succeeded = true;
                    break;
                }
            }
            assert!(succeeded, "operation {operation} never succeeded");
        }
    }
}

#[test]
fn invalid_abi_shapes_are_rejected_before_dereference() {
    let p = std::ptr::null_mut();
    assert_eq!(
        unsafe { plla_v1_multiply(-1, 2, 2, p, 0, 8, p, 0, 8, p, 0, 8) },
        SHAPE
    );
    assert_eq!(unsafe { plla_v1_cholesky(2, p, 4, 8) }, SHAPE);
    assert_eq!(unsafe { plla_v1_cholesky(i64::MAX, p, 0, 8) }, SHAPE);
}

fn product(a: &[f64], m: usize, k: usize, b: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![0.0; m * n];
    for col in 0..n {
        for row in 0..m {
            for inner in 0..k {
                out[col * m + row] += a[inner * m + row] * b[col * k + inner];
            }
        }
    }
    out
}

fn close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len());
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        assert!(
            (x - y).abs() <= 1e-9 * (1.0 + x.abs()),
            "element {i}: {x} != {y}"
        );
    }
}

#[test]
fn blocked_decompositions_have_small_residuals() {
    for (m, n) in [(17, 65), (65, 17), (65, 65), (129, 65)] {
        let mut a = vec![0.0; m * n];
        for j in 0..n {
            for i in 0..m {
                a[j * m + i] =
                    ((i * 13 + j * 7 + 3) % 23) as f64 / 23.0 + if i == j { 2.0 } else { 0.0 };
            }
        }
        let rank = m.min(n);
        let mut packed = a.clone();
        let mut p = vec![0i64; m];
        assert_eq!(
            unsafe {
                plla_v1_lu(
                    m as i64,
                    n as i64,
                    packed.as_mut_ptr().cast(),
                    packed.len(),
                    8,
                    p.as_mut_ptr().cast(),
                    p.len(),
                    8,
                )
            },
            0
        );
        let mut l = vec![0.0; m * rank];
        let mut r = vec![0.0; rank * n];
        let mut pa = vec![0.0; m * n];
        for j in 0..rank {
            for i in j..m {
                l[j * m + i] = if i == j { 1.0 } else { packed[j * m + i] };
            }
        }
        for j in 0..n {
            for i in 0..rank.min(j + 1) {
                r[j * rank + i] = packed[j * m + i];
            }
        }
        for j in 0..n {
            for i in 0..m {
                pa[j * m + i] = a[j * m + p[i] as usize];
            }
        }
        close(&product(&l, m, rank, &r, n), &pa);
        let mut q = vec![0.0; m * m];
        for i in 0..m {
            q[i * m + i] = 1.0;
        }
        packed.copy_from_slice(&a);
        assert_eq!(
            unsafe {
                plla_v1_qr(
                    m as i64,
                    n as i64,
                    packed.as_mut_ptr().cast(),
                    packed.len(),
                    8,
                    q.as_mut_ptr().cast(),
                    q.len(),
                    8,
                )
            },
            0
        );
        for j in 0..n {
            for i in j + 1..m {
                packed[j * m + i] = 0.0;
            }
        }
        close(&product(&q, m, m, &packed, n), &a);
        let mut s = vec![0.0; rank];
        let mut u = vec![0.0; m * rank];
        let mut v = vec![0.0; n * rank];
        assert_eq!(
            unsafe {
                plla_v1_svd(
                    m as i64,
                    n as i64,
                    a.as_ptr().cast(),
                    a.len(),
                    8,
                    s.as_mut_ptr().cast(),
                    s.len(),
                    8,
                    u.as_mut_ptr().cast(),
                    u.len(),
                    8,
                    v.as_mut_ptr().cast(),
                    v.len(),
                    8,
                )
            },
            0
        );
        let mut svt = vec![0.0; rank * n];
        for j in 0..n {
            for i in 0..rank {
                svt[j * rank + i] = s[i] * v[i * n + j];
            }
        }
        close(&product(&u, m, rank, &svt, n), &a);
    }
}

#[test]
fn strided_product_matches_independent_scalar_sum() {
    let (m, n, k) = (67, 71, 129);
    let a: Vec<_> = (0..m * k).map(|i| ((i * 13) % 19) as f64 / 19.0).collect();
    let b: Vec<_> = (0..k * n).map(|i| ((i * 7) % 31) as f64 / 31.0).collect();
    let expected = product(&a, m, k, &b, n);
    let mut av = vec![123.0; 2 * a.len()];
    let mut bv = vec![123.0; 2 * b.len()];
    let mut out = vec![123.0; 2 * m * n];
    for (i, &x) in a.iter().enumerate() {
        av[2 * i] = x;
    }
    for (i, &x) in b.iter().enumerate() {
        bv[2 * i] = x;
    }
    assert_eq!(
        unsafe {
            plla_v1_multiply(
                m as i64,
                n as i64,
                k as i64,
                av.as_ptr().cast(),
                a.len(),
                16,
                bv.as_ptr().cast(),
                b.len(),
                16,
                out.as_mut_ptr().cast(),
                m * n,
                16,
            )
        },
        0
    );
    let actual: Vec<_> = out
        .chunks(2)
        .map(|x| {
            assert_eq!(x[1], 123.0);
            x[0]
        })
        .collect();
    close(&actual, &expected);
}
