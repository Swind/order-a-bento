#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome<I> {
    Created(I),
    Updated(I),
    Unchanged(I),
}

/// Per-input outcomes, in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertReport<I> {
    pub outcomes: Vec<UpsertOutcome<I>>,
}

impl<I> UpsertReport<I> {
    fn count(&self, f: impl Fn(&UpsertOutcome<I>) -> bool) -> usize {
        self.outcomes.iter().filter(|o| f(o)).count()
    }
    pub fn created(&self) -> usize {
        self.count(|o| matches!(o, UpsertOutcome::Created(_)))
    }
    pub fn updated(&self) -> usize {
        self.count(|o| matches!(o, UpsertOutcome::Updated(_)))
    }
    pub fn unchanged(&self) -> usize {
        self.count(|o| matches!(o, UpsertOutcome::Unchanged(_)))
    }
}
