//! `cv2.kmeans(data, K, None, (EPS + MAX_ITER, 10, 1.0), attempts, KMEANS_PP_CENTERS)` after `cv2.setRNGSeed(0)`, as
//! OpenCV 5.0.0 runs it on one thread (core/src/kmeans.cpp, rand.cpp): the multiply-with-carry RNG, k-means++ with
//! three trials, float sums in sample order and the double compactness that picks the attempt.

/// cv::RNG: `state = (u32)state * 4164903690 + (state >> 32)`.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0xffff_ffff } else { seed })
    }
    fn next(&mut self) -> u32 {
        self.0 = (self.0 as u32 as u64).wrapping_mul(4164903690).wrapping_add(self.0 >> 32);
        self.0 as u32
    }
    fn uniform(&mut self) -> f64 {
        let t = self.next() as u64;
        ((t << 32) | self.next() as u64) as f64 * 5.4210108624275221700372640043497e-20
    }
}

/// hal::normL2Sqr_ for three floats: the vector loop never runs, the tail adds in order from 0.
fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let mut d = 0f32;
    for j in 0..3 {
        let t = a[j] - b[j];
        d += t * t;
    }
    d
}

fn centers_pp(data: &[[f32; 3]], k_count: usize, rng: &mut Rng, trials: usize) -> Vec<[f32; 3]> {
    let n = data.len();
    let mut centers = vec![0usize; k_count];
    let mut d = vec![0f32; n];
    let mut td = vec![0f32; n];
    let mut td2 = vec![0f32; n];
    let mut sum0 = 0f64;
    centers[0] = rng.next() as usize % n;
    for i in 0..n {
        d[i] = dist(&data[i], &data[centers[0]]);
        sum0 += d[i] as f64;
    }
    for k in 1..k_count {
        let mut best_sum = f64::MAX;
        let mut best_center: i64 = -1;
        for _ in 0..trials {
            let mut p = rng.uniform() * sum0;
            let mut ci = 0;
            while ci < n - 1 {
                p -= d[ci] as f64;
                if p <= 0.0 {
                    break;
                }
                ci += 1;
            }
            for i in 0..n {
                td2[i] = dist(&data[i], &data[ci]).min(d[i]);
            }
            let mut s = 0f64;
            let mut i = 0;
            while i + 7 < n {
                s += (td2[i] + td2[i + 1] + td2[i + 2] + td2[i + 3] + td2[i + 4] + td2[i + 5] + td2[i + 6] + td2[i + 7]) as f64;
                i += 8;
            }
            while i < n {
                s += td2[i] as f64;
                i += 1;
            }
            if s < best_sum {
                best_sum = s;
                best_center = ci as i64;
                std::mem::swap(&mut td, &mut td2);
            }
        }
        centers[k] = best_center.max(0) as usize;
        sum0 = best_sum;
        std::mem::swap(&mut d, &mut td);
    }
    centers.iter().map(|&c| data[c]).collect()
}

/// The labels and centres of the attempt with the lowest compactness.
pub fn kmeans(data: &[[f32; 3]], k_count: usize, max_count: i32, epsilon: f64, attempts: usize, rng: &mut Rng) -> (Vec<usize>, Vec<[f32; 3]>) {
    let n = data.len();
    let epsilon = epsilon.max(0.0).powi(2);
    let (mut attempts, mut max_count) = (attempts.max(1), max_count.clamp(2, 100));
    if k_count == 1 {
        attempts = 1;
        max_count = 2;
    }
    let mut labels = vec![0usize; n];
    let mut dists = vec![0f64; n];
    let mut best = (f64::MAX, Vec::new(), Vec::new());
    for _ in 0..attempts {
        let mut centers = vec![[0f32; 3]; k_count];
        let mut old_centers;
        let mut compactness;
        let mut iter = 0;
        loop {
            let mut max_shift = if iter == 0 { f64::MAX } else { 0.0 };
            old_centers = centers.clone();
            if iter == 0 {
                centers = centers_pp(data, k_count, rng, 3);
            } else {
                centers = vec![[0f32; 3]; k_count];
                let mut counters = vec![0i64; k_count];
                for i in 0..n {
                    let c = &mut centers[labels[i]];
                    for j in 0..3 {
                        c[j] += data[i][j];
                    }
                    counters[labels[i]] += 1;
                }
                for k in 0..k_count {
                    if counters[k] != 0 {
                        continue;
                    }
                    let mut max_k = 0;
                    for k1 in 1..k_count {
                        if counters[max_k] < counters[k1] {
                            max_k = k1;
                        }
                    }
                    let scale = 1.0f32 / counters[max_k] as f32;
                    let base: [f32; 3] = [centers[max_k][0] * scale, centers[max_k][1] * scale, centers[max_k][2] * scale];
                    let (mut max_dist, mut farthest) = (0f64, 0usize);
                    for i in 0..n {
                        if labels[i] != max_k {
                            continue;
                        }
                        let dd = dist(&data[i], &base) as f64;
                        if max_dist <= dd {
                            max_dist = dd;
                            farthest = i;
                        }
                    }
                    counters[max_k] -= 1;
                    counters[k] += 1;
                    labels[farthest] = k;
                    for j in 0..3 {
                        centers[max_k][j] -= data[farthest][j];
                        centers[k][j] += data[farthest][j];
                    }
                }
                for k in 0..k_count {
                    let scale = 1.0f32 / counters[k] as f32;
                    for j in 0..3 {
                        centers[k][j] *= scale;
                    }
                    if iter > 0 {
                        let mut d = 0f64;
                        for j in 0..3 {
                            let t = (centers[k][j] - old_centers[k][j]) as f64;
                            d += t * t;
                        }
                        max_shift = max_shift.max(d);
                    }
                }
            }
            iter += 1;
            if iter == max_count.max(2) || max_shift <= epsilon {
                for i in 0..n {
                    dists[i] = dist(&data[i], &centers[labels[i]]) as f64;
                }
                compactness = 0f64;
                let mut i = 0;
                while i + 4 <= n {
                    compactness += dists[i] + dists[i + 1] + dists[i + 2] + dists[i + 3];
                    i += 4;
                }
                while i < n {
                    compactness += dists[i];
                    i += 1;
                }
                break;
            }
            for i in 0..n {
                let (mut k_best, mut min_dist) = (0, f64::MAX);
                for (k, c) in centers.iter().enumerate() {
                    let d = dist(&data[i], c) as f64;
                    if min_dist > d {
                        min_dist = d;
                        k_best = k;
                    }
                }
                dists[i] = min_dist;
                labels[i] = k_best;
            }
        }
        if compactness < best.0 {
            best = (compactness, labels.clone(), centers.clone());
        }
    }
    (best.1, best.2)
}
