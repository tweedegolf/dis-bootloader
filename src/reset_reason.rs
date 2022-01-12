/// Reason why the device was reset.
#[derive(Debug, PartialEq)]
pub enum ResetReason {
    ResetPin,
    Watchdog,
    WakeupFromOff,
    DebugFromOff,
    SysResetReq,
    Lockup,
    CtrlAp,
    None,
}

impl ResetReason {
    pub fn lookup(power: &embassy_nrf::pac::power::RegisterBlock) -> Self {
        let reason = power.resetreas.read();
        if reason.resetpin().is_detected() {
            return ResetReason::ResetPin;
        }
        if reason.dog().is_detected() {
            return ResetReason::Watchdog;
        }
        if reason.off().is_detected() {
            return ResetReason::WakeupFromOff;
        }
        if reason.dif().is_detected() {
            return ResetReason::DebugFromOff;
        }
        if reason.sreq().is_detected() {
            return ResetReason::SysResetReq;
        }
        if reason.lockup().is_detected() {
            return ResetReason::Lockup;
        }
        if reason.ctrlap().is_detected() {
            return ResetReason::CtrlAp;
        }
        ResetReason::None
    }

    pub fn clear(power: &embassy_nrf::pac::power::RegisterBlock) {
        power.resetreas.write(|w| {
            w.resetpin()
                .set_bit()
                .dog()
                .set_bit()
                .off()
                .set_bit()
                .dif()
                .set_bit()
                .sreq()
                .set_bit()
                .lockup()
                .set_bit()
                .ctrlap()
                .set_bit()
        })
    }
}

impl core::fmt::Display for ResetReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}
