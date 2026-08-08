use super::super::*;

#[derive(Debug)]
pub(crate) struct CompactHiddenHistoryCell {
    inner: PlainHistoryCell,
}

impl CompactHiddenHistoryCell {
    pub(crate) fn new(inner: PlainHistoryCell) -> Self {
        Self { inner }
    }
}

impl HistoryCell for CompactHiddenHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.inner.display_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.inner.raw_lines()
    }

    fn display_lines_for_mode(&self, width: u16, mode: HistoryRenderMode) -> Vec<Line<'static>> {
        match mode {
            HistoryRenderMode::CompactToolActivity => Vec::new(),
            HistoryRenderMode::Rich => self.display_lines(width),
            HistoryRenderMode::Raw => self.raw_lines(),
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.inner.transcript_lines(width)
    }
}
