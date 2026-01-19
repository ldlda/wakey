use std::iter::Fuse;

/// wrap ts into a [Fuse][std::iter::Fuse] or something
pub struct Real<A: Clone, B: Iterator<Item = A>> {
    prepend: A,
    iter: B,
    my_turn: bool,
}

impl<A: Clone, B: Iterator<Item = A>> Real<A, B> {
    pub fn new(prepend: A, iter: B) -> Self {
        Self {
            prepend,
            iter,
            my_turn: true,
        }
    }
    pub fn fuse(prepend: A, iter: B) -> Fuse<Self> {
        Self::new(prepend, iter).fuse()
    }
}

impl<A: Clone, B: Iterator<Item = A>> Iterator for Real<A, B> {
    type Item = A;

    fn next(&mut self) -> Option<Self::Item> {
        let a = if self.my_turn {
            Some(self.prepend.clone())
        } else {
            self.iter.next()
        };
        self.my_turn = !self.my_turn;
        a
    }
}
