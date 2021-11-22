use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bitfield::bitfield;

use crate::{core::CoreRegister, DebugProbeError, Memory};

use super::ArmDebugSequence;

pub struct RP2040(pub(crate) ());

impl RP2040 {
    pub fn create() -> Arc<dyn ArmDebugSequence> {
        Arc::new(RP2040(()))
    }
}

impl ArmDebugSequence for RP2040 {
    /// Executes a system-wide reset without debug domain (or warm-reset that preserves debug connection) via software mechanisms,
    /// for example AIRCR.SYSRESETREQ.  This is based on the
    /// `ResetSystem` function from the [ARM SVD Debug Description].
    ///
    /// [ARM SVD Debug Description]: http://www.keil.com/pack/doc/cmsis/Pack/html/debug_description.html#resetSystem
    #[doc(alias = "ResetSystem")]
    fn reset_system(&self, interface: &mut Memory) -> Result<(), crate::Error> {
        use crate::architecture::arm::core::armv7m::{Aircr, Dhcsr};

        // On the RP2040, AIRCR.SYSRESETREQ only resets the core, not the
        // peripherals. Simulate a system-wide reset by resetting the other
        // core (if possible) and peripherals, as well.

        // trigger reset of other core, if possible
        const SIO_CPUID_ADDR: u32 = 0xd0000000;
        let cpuid = interface.read_word_32(SIO_CPUID_ADDR)?;
        if cpuid == 0 {
            let mut wdsel = PsmWdsel(0);
            wdsel.set_proc1(true); // only reset proc1

            let mut wdctrl = WatchdogCtrl(0);
            wdctrl.set_trigger(true);
            wdctrl.set_pause_dbg0(true);
            wdctrl.set_pause_dbg1(true);
            wdctrl.set_pause_jtag(true);

            interface.write_word_32(PsmWdsel::ADDRESS, wdsel.into())?;
            interface.write_word_32(WatchdogCtrl::ADDRESS, wdctrl.into())?;
        } else {
            log::warn!("Cannot reset CORE0 while connected to CORE1");
        }

        // reset peripherals
        // (do not reset PLL_SYS as that may feed sys_clk)
        const RESETS_RESET_ADDR: u32 = 0x4000c000;
        interface.write_word_32(RESETS_RESET_ADDR, 0x01ffefff)?;

        // trigger reset of PROC0 via sysreset
        let mut aircr = Aircr(0);
        aircr.vectkey();
        aircr.set_sysresetreq(true);

        interface.write_word_32(Aircr::ADDRESS, aircr.into())?;

        let start = Instant::now();

        while start.elapsed() < Duration::from_micros(50_0000) {
            let dhcsr = Dhcsr(interface.read_word_32(Dhcsr::ADDRESS)?);

            // Wait until the S_RESET_ST bit is cleared on a read
            if !dhcsr.s_reset_st() {
                return Ok(());
            }
        }

        Err(crate::Error::Probe(DebugProbeError::Timeout))
    }
}


bitfield! {
    #[derive(Copy, Clone)]
    pub struct PsmWdsel(u32);
    impl Debug;
    pub proc1, set_proc1: 16;
    pub proc0, set_proc0: 15;
    pub sio, set_sio: 14;
    pub vreg_and_chip_reset, set_vreg_and_chip_reset: 13;
    pub xip, set_xip: 12;
    pub sram5, set_sram5: 11;
    pub sram4, set_sram4: 10;
    pub sram3, set_sram3: 9;
    pub sram2, set_sram2: 8;
    pub sram1, set_sram1: 7;
    pub sram0, set_sram0: 6;
    pub rom, set_rom: 5;
    pub busfabric, set_busfabric: 4;
    pub resets, set_resets: 3;
    pub clocks, set_clocks: 2;
    pub xosc, set_xosc: 1;
    pub rosc, set_rosc: 0;
}

impl From<u32> for PsmWdsel {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<PsmWdsel> for u32 {
    fn from(value: PsmWdsel) -> Self {
        value.0
    }
}

impl CoreRegister for PsmWdsel {
    const ADDRESS: u32 = 0x4001_0008;
    const NAME: &'static str = "PSM_WDSEL";
}


bitfield! {
    #[derive(Copy, Clone)]
    pub struct WatchdogCtrl(u32);
    impl Debug;
    pub trigger, set_trigger: 31;
    pub pause_dbg1, set_pause_dbg1: 26;
    pub pause_dbg0, set_pause_dbg0: 25;
    pub pause_jtag, set_pause_jtag: 24;
    // TODO add fields
}

impl From<u32> for WatchdogCtrl {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<WatchdogCtrl> for u32 {
    fn from(value: WatchdogCtrl) -> Self {
        value.0
    }
}

impl CoreRegister for WatchdogCtrl {
    const ADDRESS: u32 = 0x4005_8000;
    const NAME: &'static str = "WATCHDOG_CTRL";
}
