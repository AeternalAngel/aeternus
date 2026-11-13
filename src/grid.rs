use crate::cell::Cell;
pub struct Grid {
    cells: Vec<Cell>,
    cols: usize,
    rows: usize,
    top: usize,
}
impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self { cells: vec![Cell::BLANK; cols * rows], cols, rows, top: 0 }
    }
    pub fn cols(&self) -> usize { self.cols }
    pub fn rows(&self) -> usize { self.rows }
    fn phys(&self, row: usize) -> usize { (self.top + row) % self.rows }
    pub fn row(&self, r: usize) -> &[Cell] {
        let s = self.phys(r) * self.cols;
        &self.cells[s..s + self.cols]
    }
    pub fn row_mut(&mut self, r: usize) -> &mut [Cell] {
        let s = self.phys(r) * self.cols;
        &mut self.cells[s..s + self.cols]
    }
    pub fn fill(&mut self, r: usize, from: usize, to: usize, blank: Cell) {
        self.row_mut(r)[from..to].fill(blank);
    }
    fn copy_row(&mut self, src: usize, dst: usize) {
        let s = self.phys(src) * self.cols;
        let d = self.phys(dst) * self.cols;
        self.cells.copy_within(s..s + self.cols, d);
    }
    pub fn scroll_up(&mut self, top: usize, bottom: usize, n: usize, blank: Cell) {
        let n = n.min(bottom - top + 1);
        if n == 0 { return; }
        if top == 0 && bottom == self.rows - 1 {
            for i in 0..n {
                let s = self.phys(i) * self.cols;
                self.cells[s..s + self.cols].fill(blank);
            }
            self.top = (self.top + n) % self.rows;
        } else {
            for r in top..(bottom + 1 - n) {
                self.copy_row(r + n, r);
            }
            for r in (bottom + 1 - n)..=bottom {
                self.row_mut(r).fill(blank);
            }
        }
    }
    pub fn scroll_down(&mut self, top: usize, bottom: usize, n: usize, blank: Cell) {
        let n = n.min(bottom - top + 1);
        if n == 0 { return; }
        for r in (top..(bottom + 1 - n)).rev() {
            self.copy_row(r, r + n);
        }
        for r in top..(top + n) {
            self.row_mut(r).fill(blank);
        }
    }
    pub fn resized(&self, cols: usize, rows: usize) -> Grid {
        let mut g = Grid::new(cols, rows);
        let n = cols.min(self.cols);
        for r in 0..rows.min(self.rows) {
            g.row_mut(r)[..n].copy_from_slice(&self.row(r)[..n]);
        }
        g
    }
}
