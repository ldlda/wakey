//! rare fns that belong nowhere

/// absolute dih cheese
/// took some inspo from itertools
pub fn product<A, B, T1, T2>(i1: T1, i2: T2) -> impl Iterator<Item = (A, B)>
where
    A: Clone,
    T1: IntoIterator<Item = A>,
    T2: IntoIterator<Item = B>,
    T2::IntoIter: Clone,
{
    let i2 = i2.into_iter();
    i1.into_iter()
        .flat_map(move |i1i| i2.clone().map(move |i2i| (i1i.clone(), i2i)))
}
