macro_rules! delay_cycles {
    ($cycles:expr) => {
        seq_macro::seq!(N in 0..$cycles {
            cortex_m::asm::nop();
        });
    };
}

pub(crate) use delay_cycles;
