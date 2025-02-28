macro_rules! forward_trait_method {
    (fn $mnm:ident $({$($fgen:tt)*})? (&self, $($param:ident : $pty:ty),* $(,)?) $(-> $ret:ty)?) => {
        // Works the same with both immutable and mutable receivers, let's reuse machinery
        _forward_trait_method!($mnm $({$($fgen)*})? (&mut self, $($param: $pty)*) $(-> $ret)?);
    };
    (fn $mnm:ident $({$($fgen:tt)*})? (&mut self, $($param:ident : $pty:ty),* $(,)?) $(-> $ret:ty)?) => {
        #[inline(always)]
        fn $mnm $(<$($fgen)*>)? (&mut self, $($param: $pty),*) $(-> $ret)? {
            (**self).$mnm($($param),*)
        }
    };
    (fn $mnm:ident $({$($fgen:tt)*})? (self, $($param:ident : $pty:ty),* $(,)?) $(-> $ret:ty)?) => {
        #[inline(always)]
        fn $mnm $(<$($fgen)*>)? (self, $($param: $pty),*) $(-> $ret)? {
            (*self).$mnm($($param),*)
        }
    };
    (fn $mnm:ident $({$($fgen:tt)*})? ($($param:ident : $pty:ty),* $(,)?) $(-> $ret:ty)?) => {
        #[inline(always)]
        fn $mnm $(<$($fgen)*>)? ($($param: $pty),*) $(-> $ret)? {
            T::$mnm($($param),*)
        }
    };
}
