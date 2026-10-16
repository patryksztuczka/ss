//! `Array.prototype.sort` as V8 implements it (TimSort, `array-sort.tq`).
//! OCR's reading-order comparator is not a consistent total order, so the
//! result depends on the algorithm: matching V8 keeps line order identical
//! to the Electron build. Rust's own sorts may also panic on such a comparator.

/// Sorts `items` in place. `compare` returns a JavaScript comparator number:
/// negative when `a` sorts before `b`.
pub fn sort_by<T: Clone>(items: &mut [T], compare: impl Fn(&T, &T) -> f64) {
    let length = items.len();
    if length < 2 {
        return;
    }
    let mut sort = Sort {
        work: items,
        compare: &compare,
        min_gallop: MIN_GALLOP,
        runs: Vec::new(),
    };
    let min_run = min_run_length(length);
    let mut low = 0;
    let mut remaining = length;
    while remaining != 0 {
        let mut run = sort.count_and_make_run(low, low + remaining);
        if run < min_run {
            let forced = min_run.min(remaining);
            sort.binary_insertion_sort(low, low + run, low + forced);
            run = forced;
        }
        sort.runs.push((low, run));
        sort.merge_collapse();
        low += run;
        remaining -= run;
    }
    sort.merge_force_collapse();
}

const MIN_GALLOP: usize = 7;

fn min_run_length(mut n: usize) -> usize {
    let mut r = 0;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

struct Sort<'a, T, F> {
    work: &'a mut [T],
    compare: &'a F,
    min_gallop: usize,
    /// Pending runs as (base, length).
    runs: Vec<(usize, usize)>,
}

/// Galloping searches run over either the work array or the merge buffer.
fn gallop_left<T>(
    compare: &impl Fn(&T, &T) -> f64,
    array: &[T],
    key: &T,
    base: usize,
    length: usize,
    hint: usize,
) -> usize {
    let mut last_offset: isize = 0;
    let mut offset: isize = 1;
    let hint = hint as isize;
    let length = length as isize;
    let at = |index: isize| &array[(base as isize + index) as usize];
    if compare(at(hint), key) < 0.0 {
        let max_offset = length - hint;
        while offset < max_offset {
            if compare(at(hint + offset), key) >= 0.0 {
                break;
            }
            last_offset = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_offset);
        last_offset += hint;
        offset += hint;
    } else {
        let max_offset = hint + 1;
        while offset < max_offset {
            if compare(at(hint - offset), key) < 0.0 {
                break;
            }
            last_offset = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_offset);
        let previous = last_offset;
        last_offset = hint - offset;
        offset = hint - previous;
    }
    last_offset += 1;
    while last_offset < offset {
        let middle = last_offset + ((offset - last_offset) >> 1);
        if compare(at(middle), key) < 0.0 {
            last_offset = middle + 1;
        } else {
            offset = middle;
        }
    }
    offset as usize
}

fn gallop_right<T>(
    compare: &impl Fn(&T, &T) -> f64,
    array: &[T],
    key: &T,
    base: usize,
    length: usize,
    hint: usize,
) -> usize {
    let mut last_offset: isize = 0;
    let mut offset: isize = 1;
    let hint = hint as isize;
    let length = length as isize;
    let at = |index: isize| &array[(base as isize + index) as usize];
    if compare(key, at(hint)) < 0.0 {
        let max_offset = hint + 1;
        while offset < max_offset {
            if compare(key, at(hint - offset)) >= 0.0 {
                break;
            }
            last_offset = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_offset);
        let previous = last_offset;
        last_offset = hint - offset;
        offset = hint - previous;
    } else {
        let max_offset = length - hint;
        while offset < max_offset {
            if compare(key, at(hint + offset)) < 0.0 {
                break;
            }
            last_offset = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_offset);
        last_offset += hint;
        offset += hint;
    }
    last_offset += 1;
    while last_offset < offset {
        let middle = last_offset + ((offset - last_offset) >> 1);
        if compare(key, at(middle)) < 0.0 {
            offset = middle;
        } else {
            last_offset = middle + 1;
        }
    }
    offset as usize
}

impl<T: Clone, F: Fn(&T, &T) -> f64> Sort<'_, T, F> {
    fn less(&self, a: &T, b: &T) -> bool {
        (self.compare)(a, b) < 0.0
    }

    fn count_and_make_run(&mut self, low_arg: usize, high: usize) -> usize {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run = 2;
        let descending = self.less(&self.work[low], &self.work[low - 1]);
        let mut previous = low;
        for index in low + 1..high {
            let order_less = self.less(&self.work[index], &self.work[previous]);
            if descending {
                if !order_less {
                    break;
                }
            } else if order_less {
                break;
            }
            previous = index;
            run += 1;
        }
        if descending {
            self.work[low_arg..low_arg + run].reverse();
        }
        run
    }

    fn binary_insertion_sort(&mut self, low: usize, start: usize, high: usize) {
        let mut start = if low == start { start + 1 } else { start };
        while start < high {
            let pivot = self.work[start].clone();
            let (mut left, mut right) = (low, start);
            while left < right {
                let middle = left + ((right - left) >> 1);
                if self.less(&pivot, &self.work[middle]) {
                    right = middle;
                } else {
                    left = middle + 1;
                }
            }
            self.work[left..=start].rotate_right(1);
            self.work[left] = pivot;
            start += 1;
        }
    }

    fn run_invariant_established(&self, n: usize) -> bool {
        if n < 2 {
            return true;
        }
        self.runs[n - 2].1 > self.runs[n - 1].1 + self.runs[n].1
    }

    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if !self.run_invariant_established(n + 1) || !self.run_invariant_established(n) {
                if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.runs[n].1 <= self.runs[n + 1].1 {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    fn merge_at(&mut self, i: usize) {
        let (mut base_a, mut length_a) = self.runs[i];
        let (base_b, mut length_b) = self.runs[i + 1];
        self.runs[i].1 = length_a + length_b;
        self.runs.remove(i + 1);

        let k = gallop_right(
            self.compare,
            self.work,
            &self.work[base_b].clone(),
            base_a,
            length_a,
            0,
        );
        base_a += k;
        length_a -= k;
        if length_a == 0 {
            return;
        }
        let key = self.work[base_a + length_a - 1].clone();
        length_b = gallop_left(
            self.compare,
            self.work,
            &key,
            base_b,
            length_b,
            length_b - 1,
        );
        if length_b == 0 {
            return;
        }
        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b);
        } else {
            self.merge_high(base_a, length_a, base_b, length_b);
        }
    }

    fn merge_low(
        &mut self,
        base_a: usize,
        mut length_a: usize,
        base_b: usize,
        mut length_b: usize,
    ) {
        let temp: Vec<T> = self.work[base_a..base_a + length_a].to_vec();
        let mut dest = base_a;
        let mut cursor_temp = 0;
        let mut cursor_b = base_b;
        self.work[dest] = self.work[cursor_b].clone();
        dest += 1;
        cursor_b += 1;

        enum End {
            Succeed,
            CopyB,
        }
        let end = 'merge: {
            length_b -= 1;
            if length_b == 0 {
                break 'merge End::Succeed;
            }
            if length_a == 1 {
                break 'merge End::CopyB;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut wins_a = 0;
                let mut wins_b = 0;
                loop {
                    if self.less(&self.work[cursor_b], &temp[cursor_temp]) {
                        self.work[dest] = self.work[cursor_b].clone();
                        dest += 1;
                        cursor_b += 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 0 {
                            break 'merge End::Succeed;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[dest] = temp[cursor_temp].clone();
                        dest += 1;
                        cursor_temp += 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 1 {
                            break 'merge End::CopyB;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;

                    let key = self.work[cursor_b].clone();
                    wins_a = gallop_right(self.compare, &temp, &key, cursor_temp, length_a, 0);
                    if wins_a > 0 {
                        self.work[dest..dest + wins_a]
                            .clone_from_slice(&temp[cursor_temp..cursor_temp + wins_a]);
                        dest += wins_a;
                        cursor_temp += wins_a;
                        length_a -= wins_a;
                        if length_a == 1 {
                            break 'merge End::CopyB;
                        }
                        // Impossible with a consistent comparator, which this is not.
                        if length_a == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[dest] = self.work[cursor_b].clone();
                    dest += 1;
                    cursor_b += 1;
                    length_b -= 1;
                    if length_b == 0 {
                        break 'merge End::Succeed;
                    }

                    let key = temp[cursor_temp].clone();
                    wins_b = gallop_left(self.compare, self.work, &key, cursor_b, length_b, 0);
                    if wins_b > 0 {
                        for offset in 0..wins_b {
                            self.work[dest + offset] = self.work[cursor_b + offset].clone();
                        }
                        dest += wins_b;
                        cursor_b += wins_b;
                        length_b -= wins_b;
                        if length_b == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[dest] = temp[cursor_temp].clone();
                    dest += 1;
                    cursor_temp += 1;
                    length_a -= 1;
                    if length_a == 1 {
                        break 'merge End::CopyB;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match end {
            End::Succeed => {
                if length_a > 0 {
                    self.work[dest..dest + length_a]
                        .clone_from_slice(&temp[cursor_temp..cursor_temp + length_a]);
                }
            }
            End::CopyB => {
                // The last element of run A belongs at the end of the merge.
                for offset in 0..length_b {
                    self.work[dest + offset] = self.work[cursor_b + offset].clone();
                }
                self.work[dest + length_b] = temp[cursor_temp].clone();
            }
        }
    }

    fn merge_high(
        &mut self,
        base_a: usize,
        mut length_a: usize,
        base_b: usize,
        mut length_b: usize,
    ) {
        let temp: Vec<T> = self.work[base_b..base_b + length_b].to_vec();
        // Signed cursors: they legitimately step to -1 past the start.
        let mut dest = (base_b + length_b - 1) as isize;
        let mut cursor_temp = length_b as isize - 1;
        let mut cursor_a = (base_a + length_a - 1) as isize;
        let w = |index: isize| index as usize;
        self.work[w(dest)] = self.work[w(cursor_a)].clone();
        dest -= 1;
        cursor_a -= 1;

        enum End {
            Succeed,
            CopyA,
        }
        let end = 'merge: {
            length_a -= 1;
            if length_a == 0 {
                break 'merge End::Succeed;
            }
            if length_b == 1 {
                break 'merge End::CopyA;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut wins_a = 0;
                let mut wins_b = 0;
                loop {
                    if self.less(&temp[w(cursor_temp)], &self.work[w(cursor_a)]) {
                        self.work[w(dest)] = self.work[w(cursor_a)].clone();
                        dest -= 1;
                        cursor_a -= 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 0 {
                            break 'merge End::Succeed;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[w(dest)] = temp[w(cursor_temp)].clone();
                        dest -= 1;
                        cursor_temp -= 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 1 {
                            break 'merge End::CopyA;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;

                    let key = temp[w(cursor_temp)].clone();
                    let k = gallop_right(
                        self.compare,
                        self.work,
                        &key,
                        base_a,
                        length_a,
                        length_a - 1,
                    );
                    wins_a = length_a - k;
                    if wins_a > 0 {
                        dest -= wins_a as isize;
                        cursor_a -= wins_a as isize;
                        for offset in (0..wins_a).rev() {
                            self.work[w(dest + 1) + offset] =
                                self.work[w(cursor_a + 1) + offset].clone();
                        }
                        length_a -= wins_a;
                        if length_a == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[w(dest)] = temp[w(cursor_temp)].clone();
                    dest -= 1;
                    cursor_temp -= 1;
                    length_b -= 1;
                    if length_b == 1 {
                        break 'merge End::CopyA;
                    }

                    let key = self.work[w(cursor_a)].clone();
                    let k = gallop_left(self.compare, &temp, &key, 0, length_b, length_b - 1);
                    wins_b = length_b - k;
                    if wins_b > 0 {
                        dest -= wins_b as isize;
                        cursor_temp -= wins_b as isize;
                        self.work[w(dest + 1)..w(dest + 1) + wins_b].clone_from_slice(
                            &temp[w(cursor_temp + 1)..w(cursor_temp + 1) + wins_b],
                        );
                        length_b -= wins_b;
                        if length_b == 1 {
                            break 'merge End::CopyA;
                        }
                        // Impossible with a consistent comparator, which this is not.
                        if length_b == 0 {
                            break 'merge End::Succeed;
                        }
                    }
                    self.work[w(dest)] = self.work[w(cursor_a)].clone();
                    dest -= 1;
                    cursor_a -= 1;
                    length_a -= 1;
                    if length_a == 0 {
                        break 'merge End::Succeed;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match end {
            End::Succeed => {
                if length_b > 0 {
                    let start = w(dest) + 1 - length_b;
                    self.work[start..start + length_b].clone_from_slice(&temp[..length_b]);
                }
            }
            End::CopyA => {
                // The first element of run B belongs at the front of the merge.
                dest -= length_a as isize;
                cursor_a -= length_a as isize;
                for offset in (0..length_a).rev() {
                    self.work[w(dest + 1) + offset] = self.work[w(cursor_a + 1) + offset].clone();
                }
                self.work[w(dest)] = temp[w(cursor_temp)].clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_consistently_like_a_stable_sort() {
        let mut state = 0x2545_f491_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for length in [0, 1, 2, 3, 10, 31, 32, 33, 63, 64, 65, 100, 257, 1000] {
            let items: Vec<(u64, usize)> = (0..length).map(|index| (next() % 20, index)).collect();
            let mut ours = items.clone();
            sort_by(&mut ours, |a, b| a.0 as f64 - b.0 as f64);
            let mut expected = items;
            expected.sort_by_key(|item| item.0);
            assert_eq!(ours, expected, "length {length}");
        }
    }

    #[test]
    fn handles_descending_and_presorted_runs() {
        let mut descending: Vec<i32> = (0..200).rev().collect();
        sort_by(&mut descending, |a, b| f64::from(a - b));
        assert_eq!(descending, (0..200).collect::<Vec<_>>());
        let mut sawtooth: Vec<i32> = (0..300).map(|value| value % 37).collect();
        let mut expected = sawtooth.clone();
        expected.sort();
        sort_by(&mut sawtooth, |a, b| f64::from(a - b));
        assert_eq!(sawtooth, expected);
    }
}
