// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Interface to the common MANA commands.
use crate::queues::Cq;
use crate::queues::Doorbell;
use crate::queues::DoorbellPage;
use crate::queues::Eq;
use crate::queues::Wq;
use crate::resources::Resource;
use crate::resources::ResourceArena;
use crate::save_restore::DoorbellSavedState;
use crate::save_restore::GdmaDriverSavedState;
use crate::save_restore::InterruptCanarySavedState;
use crate::save_restore::SavedMemoryState;
use anyhow::Context;
use futures::FutureExt;
use gdma_defs::Cqe;
use gdma_defs::DRIVER_CAP_FLAG_1_HW_VPORT_LINK_AWARE;
use gdma_defs::DRIVER_CAP_FLAG_1_HWC_TIMEOUT_RECONFIG;
use gdma_defs::DRIVER_CAP_FLAG_1_SELF_RESET_ON_EQE_NOTIFICATION;
use gdma_defs::DRIVER_CAP_FLAG_1_VARIABLE_INDIRECTION_TABLE_SUPPORT;
use gdma_defs::DRIVER_CAP_FLAG_1_VTL2_INTERRUPT_CANARY;
use gdma_defs::DRIVER_CAP_FLAG_1_VTL2_REVOKE_SUB_ON_RESET_EQE;
use gdma_defs::DRIVER_CAP_FLAG_1_VTL2_SELECTIVE_REVOKE_SUB_ON_RESET_EQE;
use gdma_defs::EqeDataReconfig;
use gdma_defs::EqeVfReset;
use gdma_defs::EstablishHwc;
use gdma_defs::GDMA_EQE_COMPLETION;
use gdma_defs::GDMA_EQE_HWC_INIT_DATA;
use gdma_defs::GDMA_EQE_HWC_INIT_DONE;
use gdma_defs::GDMA_EQE_HWC_INIT_EQ_ID_DB;
use gdma_defs::GDMA_EQE_HWC_RECONFIG_DATA;
use gdma_defs::GDMA_EQE_HWC_RESET_REQUEST;
use gdma_defs::GDMA_EQE_TEST_EVENT;
use gdma_defs::GDMA_MESSAGE_V1;
use gdma_defs::GDMA_PAGE_TYPE_4K;
use gdma_defs::GDMA_PF_CAP_FLAG_2_VTL2_INTERRUPT_CANARY;
use gdma_defs::GDMA_STANDARD_HEADER_TYPE;
use gdma_defs::GdmaChangeMsixVectorIndexForEq;
use gdma_defs::GdmaConfigureVtl2InterruptCanaryReq;
use gdma_defs::GdmaCreateDmaRegionReq;
use gdma_defs::GdmaCreateDmaRegionResp;
use gdma_defs::GdmaCreateQueueReq;
use gdma_defs::GdmaCreateQueueResp;
use gdma_defs::GdmaDestroyDmaRegionReq;
use gdma_defs::GdmaDevId;
use gdma_defs::GdmaDisableQueueReq;
#[cfg(test)]
use gdma_defs::GdmaGenerateResetEventReq;
use gdma_defs::GdmaGenerateTestEventReq;
use gdma_defs::GdmaListDevicesResp;
use gdma_defs::GdmaMsgHdr;
use gdma_defs::GdmaQueryMaxResourcesResp;
use gdma_defs::GdmaQueueType;
use gdma_defs::GdmaRegisterDeviceResp;
use gdma_defs::GdmaReqHdr;
use gdma_defs::GdmaRequestType;
use gdma_defs::GdmaRespHdr;
use gdma_defs::GdmaVerifyVerReq;
use gdma_defs::GdmaVerifyVerResp;
use gdma_defs::HWC_DATA_CONFIG_HWC_TIMEOUT;
use gdma_defs::HWC_DATA_TYPE_HW_VPORT_LINK_CONNECT;
use gdma_defs::HWC_DATA_TYPE_HW_VPORT_LINK_DISCONNECT;
use gdma_defs::HWC_DEV_ID;
use gdma_defs::HWC_INIT_DATA_CQID;
use gdma_defs::HWC_INIT_DATA_GPA_MKEY;
use gdma_defs::HWC_INIT_DATA_PDID;
use gdma_defs::HWC_INIT_DATA_RQID;
use gdma_defs::HWC_INIT_DATA_SQID;
use gdma_defs::HwcInitEqIdDb;
use gdma_defs::HwcInitTypeData;
use gdma_defs::HwcTxOob;
use gdma_defs::HwcTxOobFlags3;
use gdma_defs::HwcTxOobFlags4;
use gdma_defs::RegMap;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_LEGACY_PROBE;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_PENDING_MS_MASK;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_EQE_PENDING_AFTER_INTERRUPT;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_EQE_PENDING_NO_INTERRUPT;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_INTERRUPT_NO_EQE;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_SEQUENCE_MISMATCH;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_SHIFT;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_UNEXPECTED_EQE;
use gdma_defs::SMC_GDMA_VTL2_INTERRUPT_CANARY_VALID;
use gdma_defs::SMC_MSG_TYPE_DESTROY_HWC_VERSION;
use gdma_defs::SMC_MSG_TYPE_ESTABLISH_HWC_VERSION;
use gdma_defs::SMC_MSG_TYPE_REPORT_HWC_TIMEOUT_VERSION;
use gdma_defs::SMC_MSG_TYPE_REPORT_VTL2_INTERRUPT_CANARY_VERSION;
use gdma_defs::Sge;
use gdma_defs::SmcMessageType;
use gdma_defs::SmcProtoHdr;
use inspect::Inspect;
use pal_async::driver::Driver;
use pal_async::timer::PolledTimer;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::time::Duration;
use user_driver::DeviceBacking;
use user_driver::DeviceRegisterIo;
use user_driver::backoff::Backoff;
use user_driver::interrupt::DeviceInterrupt;
use user_driver::memory::MemoryBlock;
use user_driver::memory::PAGE_SIZE;
use user_driver::memory::PAGE_SIZE64;
use zerocopy::FromBytes;
use zerocopy::FromZeros;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;

const HWC_WARNING_TIME_IN_MS: u32 = 3000;
const HWC_WARNING_INCREASE_IN_MS: u32 = 1000;
const HWC_TIMEOUT_DEFAULT_IN_MS: u32 = 10000;
const HWC_TIMEOUT_FOR_SHUTDOWN_IN_MS: u32 = 100;
const HWC_POLL_TIMEOUT_IN_MS: u64 = 10000;
const HWC_INTERRUPT_POLL_WAIT_MIN_MS: u32 = 20;
const HWC_INTERRUPT_POLL_WAIT_MAX_MS: u32 = 500;
pub(crate) const VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS: u32 = 10;
const VTL2_INTERRUPT_CANARY_MIN_DELAY_MS: u32 = 250;
const VTL2_INTERRUPT_CANARY_MAX_DELAY_MS: u32 = 750;
const VTL2_INTERRUPT_CANARY_COMPLETION_TIMEOUT_MS: u32 = 500;
const VTL2_INTERRUPT_CANARY_PENDING_POLLS: u32 = 25;
const VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS: u32 = 100;
const VTL2_INTERRUPT_CANARY_MAX_CONSECUTIVE_POLL_RECOVERIES: u32 = 32;
const VTL2_INTERRUPT_CANARY_SHMEM_TIMEOUT_MS: u64 = 100;
const VTL2_INTERRUPT_CANARY_SHMEM_POLL_MS: u64 = 1;
pub(crate) const VTL2_INTERRUPT_CANARY_REPORT_RETRY_INTERVAL_MS: u64 = 500;
pub(crate) const VTL2_INTERRUPT_CANARY_REPORT_RETRY_TIMEOUT_MS: u64 = 30000;

#[derive(Inspect)]
struct Bar0<T: Inspect> {
    mem: T,
    map: RegMap,
    doorbell_shift: u32,
}

impl<T: DeviceRegisterIo + Inspect> Doorbell for Bar0<T> {
    fn page_count(&self) -> u32 {
        self.mem
            .len()
            .saturating_sub(self.map.vf_db_pages_zone_offset as usize) as u32
            >> self.doorbell_shift
    }

    fn write(&self, page_number: u32, address: u32, value: u64) {
        let offset = self.map.vf_db_pages_zone_offset
            + ((page_number as u64) << self.doorbell_shift)
            + address as u64;
        tracing::trace!(page_number, address, offset, value, "doorbell");

        // Ensure the doorbell write is ordered after the writes to the queues.
        safe_intrinsics::store_fence();
        self.mem.write_u64(offset as usize, value);
    }

    fn save(&self, doorbell_id: Option<u64>) -> DoorbellSavedState {
        DoorbellSavedState {
            doorbell_id: doorbell_id.unwrap(),
            page_count: self.page_count(),
        }
    }
}

#[derive(Inspect)]
pub struct GdmaDriver<T: DeviceBacking> {
    device: Option<T>,
    bar0: Arc<Bar0<T::Registers>>,
    #[inspect(skip)]
    shmem_poll_timer: PolledTimer,
    #[inspect(skip)]
    dma_buffer: MemoryBlock,
    #[inspect(skip)]
    interrupts: Vec<Option<DeviceInterrupt>>,
    eq: Eq,
    cq: Cq,
    rq: Wq,
    sq: Wq,
    test_events: u64,
    eq_armed: bool,
    cq_armed: bool,
    gpa_mkey: u32,
    _pdid: u32,
    #[inspect(iter_by_key)]
    eq_id_msix: HashMap<u32, u32>,
    num_msix: u32,
    max_msix_available: u32,
    min_queue_avail: u32,
    hwc_activity_id: u32,
    #[inspect(skip)]
    link_toggle: Vec<(u32, bool)>,
    hwc_subscribed: bool,
    hwc_warning_time_in_ms: u32,
    hwc_timeout_in_ms: u32,
    hwc_failure: bool,
    vtl2_interrupt_canary_supported: bool,
    vtl2_interrupt_canary_reserved: bool,
    db_id: u32,
    state_saved: bool,
    // The option will be set if there is a pending VF reset event. The
    // option value indicates whether to remove the subordinate VF or not.
    reset_request_pending: Option<bool>,
}

const EQ_PAGE: usize = 0;
const CQ_PAGE: usize = 1;
const RQ_PAGE: usize = 2;
const SQ_PAGE: usize = 3;
const REQUEST_PAGE: usize = 4;
const RESPONSE_PAGE: usize = 5;
const VTL2_INTERRUPT_CANARY_EQ_PAGE: usize = 6;
const NUM_PAGES: usize = 7;

// RWQEs have no OOB and one SGL entry so they are always exactly 32 bytes.
const RWQE_SIZE: u32 = 32;

impl<T: DeviceBacking> GdmaDriver<T> {
    /// Polls the shared‐memory ownership bit until PF gives it back (or we timeout / device not present).
    /// Returns `Ok(header)` if we successfully see VF ownership (i.e. PF bit cleared),
    /// or `Err` if the device not present or we hit our timeout.
    fn wait_for_vf_to_own_shmem(&self) -> Result<SmcProtoHdr, anyhow::Error> {
        let timeout = std::time::Instant::now() + Duration::from_millis(HWC_POLL_TIMEOUT_IN_MS);

        loop {
            let offset = self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 28;
            let data = self.bar0.mem.read_u32(offset);

            if data == u32::MAX {
                return Err(anyhow::anyhow!("Device no longer present"));
            }

            let header = SmcProtoHdr::from(data);
            if !header.owner_is_pf() {
                return Ok(header);
            }

            if std::time::Instant::now() > timeout {
                return Err(anyhow::anyhow!(
                    "MANA request timed out waiting for PF ownership to clear"
                ));
            }

            std::hint::spin_loop();
        }
    }
}

impl<T: DeviceBacking> Drop for GdmaDriver<T> {
    fn drop(&mut self) {
        tracing::info!(?self.state_saved, ?self.hwc_failure, ?self.reset_request_pending, "dropping gdma driver");

        if self.reset_request_pending.is_some() {
            return;
        }

        // Don't destroy anything if we're saving its state for restoration.
        if self.state_saved {
            // Unmap interrupts to prevent the device from sending interrupts during save/restore
            if let Err(e) = self.unmap_all_interrupts() {
                tracing::warn!(error = %e, "failed to unmap interrupts when dropping GdmaDriver");
            }

            return;
        }

        if self.hwc_failure {
            return;
        }

        // Wait for VF ownership of the shared memory before post destroy HWC
        if let Err(e) = self.wait_for_vf_to_own_shmem() {
            tracing::error!(error = %e, "Wait for VF posession to post DESTROY_HWC");
            return;
        }

        let hdr = SmcProtoHdr::new()
            .with_msg_type(SmcMessageType::SMC_MSG_TYPE_DESTROY_HWC.0)
            .with_msg_version(SMC_MSG_TYPE_DESTROY_HWC_VERSION);

        let hdr = u32::from_le_bytes(hdr.as_bytes().try_into().expect("known size"));
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 28,
            hdr,
        );

        // Wait for VF ownership of the shared memory after post destroy HWC
        match self.wait_for_vf_to_own_shmem() {
            Ok(header) => {
                if !header.is_response() {
                    tracing::error!("Unexpected response for DESTROY_HWC");
                }
                if header.status() != 0 {
                    tracing::error!(status = header.status(), "DESTROY_HWC failed");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Wait for VF possession to retrieve status after DESTROY_HWC");
            }
        }
    }
}

struct EqeWaitResult {
    eqe_found: bool,
    elapsed: u128,
    eq_arm_count: u32,
    interrupt_wait_count: u32,
    interrupt_count: u32,
    last_wait_result: anyhow::Result<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InterruptCanaryFailure {
    EqePendingNoInterrupt,
    EqePendingAfterInterrupt,
    InterruptWithoutEqe,
    UnexpectedEqe,
    SequenceMismatch,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InterruptCanaryReport {
    pub generation: u32,
    pub expected_sequence: u32,
    pub observed_sequence: u32,
    pub queue_id: u32,
    pub eq_next: u32,
    pub interrupt_count: u32,
    pub poll_count: u32,
    pub pending_ms: u32,
    pub failure: InterruptCanaryFailure,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum InterruptCanaryControl {
    RecoverPending(InterruptCanaryReport),
    ArmAfterRecoveryQuiet {
        generation: u32,
        recovered_sequence: u32,
        signal_count: u64,
    },
}

#[derive(Default)]
struct InterruptCanaryWatchdog {
    pending_event: Option<(u8, u32)>,
    pending_polls: u32,
    last_reported_event: Option<(u8, u32)>,
    poll_count: u32,
    idle_interrupt_signal_count: u64,
    pending_interrupt_signal_seen: bool,
}

impl InterruptCanaryWatchdog {
    fn clear_pending(&mut self, interrupt_signal_count: u64) {
        self.pending_event = None;
        self.pending_polls = 0;
        self.last_reported_event = None;
        self.idle_interrupt_signal_count = interrupt_signal_count;
        self.pending_interrupt_signal_seen = false;
    }

    fn observe(
        &mut self,
        event: Option<(u8, u32)>,
        interrupt_signal_count_before: u64,
        interrupt_signal_count_after: u64,
    ) -> Option<(InterruptCanaryFailure, u32, u64)> {
        self.poll_count = self.poll_count.saturating_add(1);

        let Some(event) = event else {
            self.pending_event = None;
            self.pending_polls = 0;
            self.last_reported_event = None;
            self.pending_interrupt_signal_seen = false;
            if interrupt_signal_count_before == interrupt_signal_count_after {
                self.idle_interrupt_signal_count = interrupt_signal_count_after;
            }
            return None;
        };

        if self.pending_event == Some(event) {
            self.pending_polls = self.pending_polls.saturating_add(1);
            self.pending_interrupt_signal_seen |= interrupt_signal_count_before
                > self.idle_interrupt_signal_count
                || interrupt_signal_count_after > self.idle_interrupt_signal_count;
        } else {
            self.pending_event = Some(event);
            self.pending_polls = 1;
            self.pending_interrupt_signal_seen = interrupt_signal_count_before
                > self.idle_interrupt_signal_count
                || interrupt_signal_count_after > self.idle_interrupt_signal_count;
        }

        if self.pending_polls < VTL2_INTERRUPT_CANARY_PENDING_POLLS
            || self.last_reported_event == Some(event)
        {
            return None;
        }

        self.last_reported_event = Some(event);
        let failure = if event.0 == GDMA_EQE_TEST_EVENT {
            if self.pending_interrupt_signal_seen {
                InterruptCanaryFailure::EqePendingAfterInterrupt
            } else {
                InterruptCanaryFailure::EqePendingNoInterrupt
            }
        } else {
            InterruptCanaryFailure::UnexpectedEqe
        };
        Some((
            failure,
            self.pending_polls
                .saturating_mul(VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS),
            interrupt_signal_count_after,
        ))
    }
}

pub(crate) struct InterruptCanary {
    eq: Eq,
    interrupt: DeviceInterrupt,
    resources: ResourceArena,
    msix: u32,
    generation: u32,
    expected_sequence: u32,
    interrupt_count: u32,
    watchdog: InterruptCanaryWatchdog,
    rearm_after_report: bool,
    monitoring_enabled: bool,
    poll_recovery_total: u32,
    poll_recovery_consecutive: u32,
    poll_recovery_validation_pending: bool,
    recovery_quiet_sequence: Option<u32>,
    recovery_quiet_signal_count: u64,
    recovery_quiet_polls: u32,
    recovery_arm_request_pending: bool,
    recovery_eq_armed: bool,
    recovery_quiet_counted_as_poll: bool,
}

impl InterruptCanary {
    fn next_sequence(sequence: u32) -> u32 {
        let sequence = sequence.wrapping_add(1);
        if sequence == 0 { 1 } else { sequence }
    }

    fn new(eq: Eq, interrupt: DeviceInterrupt, resources: ResourceArena, msix: u32) -> Self {
        let mut generation_bytes = [0_u8; 4];
        getrandom::fill(&mut generation_bytes).unwrap();
        let mut generation = u32::from_ne_bytes(generation_bytes);
        if generation == 0 {
            generation = 1;
        }

        let mut watchdog = InterruptCanaryWatchdog::default();
        watchdog.clear_pending(interrupt.signal_count());
        Self {
            eq,
            interrupt,
            resources,
            msix,
            generation,
            expected_sequence: 1,
            interrupt_count: 0,
            watchdog,
            rearm_after_report: false,
            monitoring_enabled: true,
            poll_recovery_total: 0,
            poll_recovery_consecutive: 0,
            poll_recovery_validation_pending: false,
            recovery_quiet_sequence: None,
            recovery_quiet_signal_count: 0,
            recovery_quiet_polls: 0,
            recovery_arm_request_pending: false,
            recovery_eq_armed: false,
            recovery_quiet_counted_as_poll: false,
        }
    }

    fn restore(eq: Eq, interrupt: DeviceInterrupt, resources: ResourceArena, msix: u32) -> Self {
        Self::new(eq, interrupt, resources, msix)
    }

    pub(crate) fn interrupt(&self) -> DeviceInterrupt {
        self.interrupt.clone()
    }

    pub(crate) fn prepare_registration(&mut self) {
        self.watchdog.clear_pending(self.interrupt.signal_count());
        self.eq.arm();
    }

    pub(crate) fn registration(&self, enable: bool) -> GdmaConfigureVtl2InterruptCanaryReq {
        GdmaConfigureVtl2InterruptCanaryReq {
            queue_index: self.eq.id(),
            enable: enable as u32,
            poll_interval_ms: VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
            min_delay_ms: VTL2_INTERRUPT_CANARY_MIN_DELAY_MS,
            max_delay_ms: VTL2_INTERRUPT_CANARY_MAX_DELAY_MS,
            completion_timeout_ms: VTL2_INTERRUPT_CANARY_COMPLETION_TIMEOUT_MS,
            generation: self.generation,
            reserved: 0,
        }
    }

    fn make_report(
        &self,
        observed_sequence: u32,
        pending_ms: u32,
        failure: InterruptCanaryFailure,
    ) -> InterruptCanaryReport {
        InterruptCanaryReport {
            generation: self.generation,
            expected_sequence: self.expected_sequence,
            observed_sequence,
            queue_id: self.eq.id(),
            eq_next: self.eq.get_next(),
            interrupt_count: self.interrupt.signal_count().try_into().unwrap_or(u32::MAX),
            poll_count: self.watchdog.poll_count,
            pending_ms,
            failure,
        }
    }

    pub(crate) fn process_interrupt(&mut self) -> Option<InterruptCanaryReport> {
        if !self.monitoring_enabled {
            return None;
        }

        self.interrupt_count = self.interrupt_count.saturating_add(1);
        let mut event_found = false;
        let mut expected_event_found = false;
        let mut failure = None;

        while let Some(eqe) = self.eq.pop() {
            event_found = true;
            let observed_sequence =
                u32::from_le_bytes(eqe.data[..4].try_into().expect("known size"));
            if eqe.params.event_type() != GDMA_EQE_TEST_EVENT {
                tracing::error!(
                    event_type = eqe.params.event_type(),
                    observed_sequence,
                    "unexpected VTL2 interrupt canary EQE"
                );
                failure = Some(self.make_report(
                    observed_sequence,
                    0,
                    InterruptCanaryFailure::UnexpectedEqe,
                ));
            } else if observed_sequence != self.expected_sequence || expected_event_found {
                tracing::error!(
                    expected_sequence = self.expected_sequence,
                    observed_sequence,
                    "mismatched VTL2 interrupt canary sequence"
                );
                failure = Some(self.make_report(
                    observed_sequence,
                    0,
                    InterruptCanaryFailure::SequenceMismatch,
                ));
            } else {
                expected_event_found = true;
                tracing::trace!(
                    sequence = observed_sequence,
                    interrupt_count = self.interrupt_count,
                    "processed VTL2 interrupt canary EQE"
                );
            }
        }

        if event_found {
            self.watchdog.clear_pending(self.interrupt.signal_count());
            if failure.is_none() && expected_event_found {
                if self.poll_recovery_validation_pending {
                    tracing::info!(
                        generation = self.generation,
                        sequence = self.expected_sequence,
                        consecutive_poll_recoveries = self.poll_recovery_consecutive,
                        total_poll_recoveries = self.poll_recovery_total,
                        signal_count = self.interrupt.signal_count(),
                        "VTL2 interrupt canary hardware delivery resumed after polling recovery"
                    );
                    self.poll_recovery_consecutive = 0;
                }
                if self.recovery_quiet_sequence.is_some() {
                    self.recovery_quiet_sequence = None;
                    self.recovery_eq_armed = false;
                    self.recovery_quiet_counted_as_poll = false;
                    self.poll_recovery_validation_pending = false;
                }
                self.expected_sequence = Self::next_sequence(self.expected_sequence);
                self.eq.arm();
            } else {
                self.rearm_after_report = true;
            }
        } else if let Some(recovered_sequence) = self.recovery_quiet_sequence {
            if self.recovery_eq_armed {
                self.eq.ack();
                self.recovery_eq_armed = false;
                self.poll_recovery_validation_pending = false;
            }
            if self.recovery_quiet_counted_as_poll {
                self.poll_recovery_total = self.poll_recovery_total.saturating_sub(1);
                self.poll_recovery_consecutive = 0;
                self.recovery_quiet_counted_as_poll = false;
            }
            self.recovery_quiet_signal_count = self.interrupt.signal_count();
            self.recovery_quiet_polls = 0;
            self.recovery_arm_request_pending = false;
            tracing::warn!(
                generation = self.generation,
                recovered_sequence,
                signal_count = self.recovery_quiet_signal_count,
                consecutive_poll_recoveries = self.poll_recovery_consecutive,
                "late VTL2 interrupt arrived during the polling-recovery quiet period"
            );
        } else {
            failure = Some(self.make_report(0, 0, InterruptCanaryFailure::InterruptWithoutEqe));
            self.rearm_after_report = true;
        }

        failure
    }

    pub(crate) fn observe(&mut self) -> Option<InterruptCanaryReport> {
        let interrupt_signal_count_before = self.interrupt.signal_count();
        let event = self.eq.peek().map(|eqe| {
            (
                eqe.params.event_type(),
                u32::from_le_bytes(eqe.data[..4].try_into().expect("known size")),
            )
        });
        let interrupt_signal_count_after = self.interrupt.signal_count();
        let expected_sequence = self.expected_sequence;
        self.watchdog
            .observe(
                event,
                interrupt_signal_count_before,
                interrupt_signal_count_after,
            )
            .map(|(failure, pending_ms, interrupt_signal_count)| {
                let observed_sequence = event.map_or(0, |(_, sequence)| sequence);
                let failure = match failure {
                    InterruptCanaryFailure::EqePendingNoInterrupt
                    | InterruptCanaryFailure::EqePendingAfterInterrupt
                        if observed_sequence != expected_sequence =>
                    {
                        InterruptCanaryFailure::SequenceMismatch
                    }
                    other => other,
                };
                let mut report = self.make_report(observed_sequence, pending_ms, failure);
                report.interrupt_count = interrupt_signal_count.try_into().unwrap_or(u32::MAX);
                report
            })
    }

    pub(crate) fn recovery_control(&mut self) -> Option<InterruptCanaryControl> {
        let recovered_sequence = self.recovery_quiet_sequence?;
        if self.recovery_eq_armed {
            return None;
        }
        let signal_count = self.interrupt.signal_count();

        if signal_count != self.recovery_quiet_signal_count {
            self.recovery_quiet_signal_count = signal_count;
            self.recovery_quiet_polls = 0;
            self.recovery_arm_request_pending = false;
            self.recovery_eq_armed = false;
            return None;
        }

        if self.recovery_arm_request_pending {
            return None;
        }

        self.recovery_quiet_polls = self.recovery_quiet_polls.saturating_add(1);
        if self.recovery_quiet_polls < VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            return None;
        }

        self.recovery_arm_request_pending = true;
        Some(InterruptCanaryControl::ArmAfterRecoveryQuiet {
            generation: self.generation,
            recovered_sequence,
            signal_count,
        })
    }

    pub(crate) fn process_control(&mut self, control: InterruptCanaryControl) {
        match control {
            InterruptCanaryControl::RecoverPending(report) => {
                self.recover_pending_after_report(report)
            }
            InterruptCanaryControl::ArmAfterRecoveryQuiet {
                generation,
                recovered_sequence,
                signal_count,
            } => self.arm_after_recovery_quiet(generation, recovered_sequence, signal_count),
        }
    }

    fn recover_pending_after_report(&mut self, report: InterruptCanaryReport) {
        if report.failure != InterruptCanaryFailure::EqePendingNoInterrupt
            || !self.monitoring_enabled
        {
            return;
        }

        if report.generation != self.generation
            || report.queue_id != self.eq.id()
            || report.expected_sequence != self.expected_sequence
            || report.observed_sequence != self.expected_sequence
        {
            tracing::warn!(
                generation = self.generation,
                report_generation = report.generation,
                expected_sequence = self.expected_sequence,
                report_expected_sequence = report.expected_sequence,
                report_observed_sequence = report.observed_sequence,
                queue_id = self.eq.id(),
                report_queue_id = report.queue_id,
                "skipping stale VTL2 interrupt canary polling-recovery request"
            );
            return;
        }

        let signal_count = self.interrupt.signal_count();
        if report.interrupt_count != u32::MAX && signal_count != u64::from(report.interrupt_count) {
            tracing::warn!(
                generation = self.generation,
                sequence = self.expected_sequence,
                report_signal_count = report.interrupt_count,
                signal_count,
                "skipping VTL2 interrupt canary polling recovery because a late signal arrived"
            );
            return;
        }

        let Some(eqe) = self.eq.peek() else {
            tracing::info!(
                generation = self.generation,
                sequence = self.expected_sequence,
                "skipping VTL2 interrupt canary polling recovery because the EQE was consumed"
            );
            return;
        };
        let observed_sequence = u32::from_le_bytes(eqe.data[..4].try_into().expect("known size"));
        if eqe.params.event_type() != GDMA_EQE_TEST_EVENT
            || observed_sequence != self.expected_sequence
        {
            tracing::error!(
                generation = self.generation,
                expected_sequence = self.expected_sequence,
                event_type = eqe.params.event_type(),
                observed_sequence,
                "VTL2 interrupt canary polling recovery found a different EQE; preserving it"
            );
            self.disable_monitoring();
            return;
        }

        if self.poll_recovery_consecutive >= VTL2_INTERRUPT_CANARY_MAX_CONSECUTIVE_POLL_RECOVERIES {
            tracing::error!(
                generation = self.generation,
                sequence = self.expected_sequence,
                consecutive_poll_recoveries = self.poll_recovery_consecutive,
                total_poll_recoveries = self.poll_recovery_total,
                "VTL2 interrupt canary polling-recovery limit reached; leaving the EQE pending"
            );
            self.disable_monitoring();
            return;
        }

        let recovered_sequence = self.expected_sequence;
        let _ = self.eq.pop().expect("peeked EQE should still be present");
        self.expected_sequence = Self::next_sequence(self.expected_sequence);
        // Publish the consumer without arming. A delayed interrupt for this EQE
        // must be observed before socmana is allowed to generate the next probe.
        self.eq.ack();
        let signal_count_after_ack = self.interrupt.signal_count();
        self.watchdog.clear_pending(signal_count_after_ack);
        self.poll_recovery_validation_pending = false;
        self.recovery_quiet_sequence = Some(recovered_sequence);
        self.recovery_quiet_signal_count = signal_count_after_ack;
        self.recovery_quiet_polls = 0;
        self.recovery_arm_request_pending = false;
        self.recovery_eq_armed = false;

        if signal_count_after_ack != signal_count {
            self.poll_recovery_consecutive = 0;
            self.recovery_quiet_counted_as_poll = false;
            tracing::warn!(
                generation = self.generation,
                recovered_sequence,
                next_expected_sequence = self.expected_sequence,
                signal_count_before_recovery = signal_count,
                signal_count_after_ack,
                "late VTL2 hardware interrupt arrived while the recovery command consumed the EQE"
            );
            return;
        }

        self.poll_recovery_total = self.poll_recovery_total.saturating_add(1);
        self.poll_recovery_consecutive = self.poll_recovery_consecutive.saturating_add(1);
        self.recovery_quiet_counted_as_poll = true;

        tracing::warn!(
            generation = self.generation,
            recovered_sequence,
            next_expected_sequence = self.expected_sequence,
            signal_count = signal_count_after_ack,
            consecutive_poll_recoveries = self.poll_recovery_consecutive,
            total_poll_recoveries = self.poll_recovery_total,
            quiet_ms =
                VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS * VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
            "poll-consumed VTL2 interrupt canary EQE after acknowledged interrupt loss"
        );
    }

    fn arm_after_recovery_quiet(
        &mut self,
        generation: u32,
        recovered_sequence: u32,
        signal_count: u64,
    ) {
        if generation != self.generation || self.recovery_quiet_sequence != Some(recovered_sequence)
        {
            return;
        }

        self.recovery_arm_request_pending = false;
        let current_signal_count = self.interrupt.signal_count();
        if current_signal_count != signal_count
            || current_signal_count != self.recovery_quiet_signal_count
        {
            self.recovery_quiet_signal_count = current_signal_count;
            self.recovery_quiet_polls = 0;
            return;
        }

        if self.eq.peek().is_some() {
            tracing::error!(
                generation = self.generation,
                recovered_sequence,
                "VTL2 interrupt canary EQ became non-empty during recovery quiet period"
            );
            self.disable_monitoring();
            return;
        }

        // Keep the recovery state active through the arm write. If the timed-out
        // interrupt arrives concurrently, disarm again and restart the quiet period.
        self.eq.arm();
        let signal_count_after_arm = self.interrupt.signal_count();
        if signal_count_after_arm != current_signal_count {
            self.eq.ack();
            self.watchdog.clear_pending(signal_count_after_arm);
            self.recovery_quiet_signal_count = signal_count_after_arm;
            self.recovery_quiet_polls = 0;
            tracing::warn!(
                generation = self.generation,
                recovered_sequence,
                signal_count_before_arm = current_signal_count,
                signal_count_after_arm,
                "late VTL2 interrupt raced with polling-recovery rearm; restarting quiet period"
            );
            return;
        }

        self.watchdog.clear_pending(signal_count_after_arm);
        self.recovery_quiet_polls = 0;
        self.poll_recovery_validation_pending = self.recovery_quiet_counted_as_poll;
        self.recovery_eq_armed = true;

        tracing::info!(
            generation = self.generation,
            recovered_sequence,
            next_expected_sequence = self.expected_sequence,
            signal_count = signal_count_after_arm,
            consecutive_poll_recoveries = self.poll_recovery_consecutive,
            total_poll_recoveries = self.poll_recovery_total,
            "VTL2 interrupt canary recovery quiet period completed; EQ rearmed"
        );
    }

    pub(crate) fn rearm_after_report(&mut self) {
        if self.rearm_after_report {
            self.eq.arm();
            self.rearm_after_report = false;
        }
    }

    pub(crate) fn disable_monitoring(&mut self) {
        self.rearm_after_report = false;
        self.monitoring_enabled = false;
        self.recovery_quiet_sequence = None;
        self.recovery_quiet_polls = 0;
        self.recovery_arm_request_pending = false;
        self.poll_recovery_validation_pending = false;
        self.recovery_eq_armed = false;
        self.recovery_quiet_counted_as_poll = false;
    }

    pub(crate) fn monitoring_enabled(&self) -> bool {
        self.monitoring_enabled
    }

    pub(crate) fn prepare_for_save(&mut self) {
        while self.eq.pop().is_some() {}
        self.rearm_after_report = false;
        self.recovery_quiet_sequence = None;
        self.recovery_quiet_polls = 0;
        self.recovery_arm_request_pending = false;
        self.poll_recovery_validation_pending = false;
        self.recovery_eq_armed = false;
        self.recovery_quiet_counted_as_poll = false;
        self.poll_recovery_consecutive = 0;
        self.watchdog.clear_pending(self.interrupt.signal_count());
        self.eq.arm();
    }

    pub(crate) fn save(self) -> InterruptCanarySavedState {
        let state = InterruptCanarySavedState {
            eq: self.eq.save(),
            msix: self.msix,
        };
        self.resources.preserve();
        state
    }

    pub(crate) fn into_resources(self) -> ResourceArena {
        self.resources
    }

    pub(crate) fn quarantine(self) {
        std::mem::forget(self);
    }
}

impl<T: DeviceBacking> GdmaDriver<T> {
    pub fn unmap_all_interrupts(&mut self) -> anyhow::Result<()> {
        let Some(device) = self.device.as_mut() else {
            return Ok(());
        };

        device.unmap_all_interrupts()
    }

    pub fn doorbell(&self) -> Arc<dyn Doorbell> {
        self.bar0.clone() as _
    }

    pub async fn new(
        driver: &impl Driver,
        mut device: T,
        num_vps: u32,
        dma_buffer: Option<MemoryBlock>,
    ) -> anyhow::Result<Self> {
        let (bar0_mapping, map) = Self::init(&mut device)?;

        // Only allocate the HWC interrupt now. Rest will be allocated later.
        let num_msix = 1;
        let mut interrupt0 = device.map_interrupt(0, 0)?;

        let dma_buffer = match dma_buffer {
            Some(buffer) => buffer,
            None => {
                let dma_client = device.dma_client();
                dma_client
                    .allocate_dma_buffer(NUM_PAGES * PAGE_SIZE)
                    .context("failed to allocate DMA buffer")?
            }
        };

        let pages = dma_buffer.pfns();

        // Write the shared memory.
        fn low(n: u64) -> [u8; 6] {
            let n = n.to_ne_bytes();
            [n[0], n[1], n[2], n[3], n[4], n[5]]
        }

        let high = ((pages[EQ_PAGE] >> 48) & 0xf)
            | ((pages[CQ_PAGE] >> 44) & 0xf0)
            | ((pages[RQ_PAGE] >> 40) & 0xf00)
            | ((pages[SQ_PAGE] >> 36) & 0xf000);

        let establish = EstablishHwc {
            eq: low(pages[EQ_PAGE]),
            cq: low(pages[CQ_PAGE]),
            rq: low(pages[RQ_PAGE]),
            sq: low(pages[SQ_PAGE]),
            high: high as u16,
            msix: 0,
            hdr: SmcProtoHdr::new()
                .with_msg_type(SmcMessageType::SMC_MSG_TYPE_ESTABLISH_HWC.0)
                .with_msg_version(SMC_MSG_TYPE_ESTABLISH_HWC_VERSION),
        };

        let shmem = <[u32]>::ref_from_bytes(establish.as_bytes()).unwrap();
        assert!(shmem.len() == 8);
        for (i, &n) in shmem.iter().enumerate() {
            bar0_mapping.write_u32(map.vf_gdma_sriov_shared_reg_start as usize + i * 4, n);
        }

        // Wait for the device to respond.
        let mut backoff = Backoff::new(driver);
        let mut ctx =
            mesh::CancelContext::new().with_timeout(Duration::from_millis(HWC_POLL_TIMEOUT_IN_MS));
        let mut hw_failure = false;
        let header = loop {
            let header = SmcProtoHdr::from(
                bar0_mapping.read_u32(map.vf_gdma_sriov_shared_reg_start as usize + 28),
            );
            if !header.owner_is_pf() {
                break header;
            }
            if hw_failure {
                anyhow::bail!("MANA request timed out. SMC_MSG_TYPE_ESTABLISH_HWC");
            }
            hw_failure = matches!(
                ctx.until_cancelled(backoff.back_off()).await,
                Err(mesh::CancelReason::DeadlineExceeded)
            );
        };

        if !header.is_response() {
            anyhow::bail!("expected response");
        }
        if header.status() != 0 {
            anyhow::bail!("establish failed: {}", header.status());
        }

        let doorbell_shift = map.vf_db_page_sz.trailing_zeros();
        let bar0 = Arc::new(Bar0 {
            mem: bar0_mapping,
            map,
            doorbell_shift,
        });

        let mut eq = Eq::new_eq(dma_buffer.subblock(0, PAGE_SIZE), DoorbellPage::null(), 0);

        let mut cq_id = None;
        let mut rq_id = None;
        let mut sq_id = None;
        let mut db_id = None;
        let mut pdid = None;
        let mut gpa_mkey = None;
        let mut eq_armed = true;
        loop {
            let eqe = loop {
                if let Some(eqe) = eq.pop() {
                    eq_armed = false;
                    break eqe;
                }
                if !eq_armed {
                    eq.arm();
                    eq_armed = true;
                    // Check if the event arrived while arming.
                    if let Some(eqe) = eq.pop() {
                        // Remove any pending interrupt events.
                        let _ = interrupt0.wait().now_or_never();
                        eq_armed = false;
                        break eqe;
                    }
                }
                tracing::debug!("waiting for eq interrupt");
                Self::wait_for_hwc_interrupt(&mut interrupt0, None, HWC_TIMEOUT_DEFAULT_IN_MS)
                    .await?;
            };
            tracing::debug!(event_type = eqe.params.event_type(), "got init eqe");
            match eqe.params.event_type() {
                GDMA_EQE_HWC_INIT_EQ_ID_DB => {
                    let data = HwcInitEqIdDb::read_from_prefix(&eqe.data[..]).unwrap().0; // TODO: zerocopy: use-rest-of-range (https://github.com/microsoft/openvmm/issues/759)
                    eq.set_id(data.eq_id().into());
                    eq.set_doorbell(DoorbellPage::new(bar0.clone(), data.doorbell().into())?);
                    db_id = Some(data.doorbell());
                }
                GDMA_EQE_HWC_INIT_DATA => {
                    let data = HwcInitTypeData::read_from_prefix(&eqe.data[..]).unwrap().0; // TODO: zerocopy: use-rest-of-range (https://github.com/microsoft/openvmm/issues/759)
                    match data.ty() {
                        HWC_INIT_DATA_CQID => cq_id = Some(data.value()),
                        HWC_INIT_DATA_RQID => rq_id = Some(data.value()),
                        HWC_INIT_DATA_SQID => sq_id = Some(data.value()),
                        HWC_INIT_DATA_GPA_MKEY => gpa_mkey = Some(data.value()),
                        HWC_INIT_DATA_PDID => pdid = Some(data.value()),
                        _ => {}
                    }
                }
                GDMA_EQE_HWC_INIT_DONE => {
                    break;
                }
                ty => anyhow::bail!("unexpected event type {}", ty),
            }
        }

        // Ack the eq now to avoid overflow. This wasn't safe to do earlier
        // because we didn't know the eq's doorbell index yet.
        eq.ack();

        // From here on, the interrupt events have moved to the msix channel
        tracing::debug!("init sequence done");

        // Start the HWC notify channel for now. Rest of the notify channels
        // will be started later once it is known how many MSI-X are actually
        // available.
        let mut eq_id_msix = HashMap::new();
        eq_id_msix.insert(eq.id(), 0);
        tracing::info!(eq_id = eq.id(), msix = 0, "created HWC");

        let db_id = db_id.context("db id not provided")? as u32;
        let gpa_mkey = gpa_mkey.context("gpa mem key not provided")?;
        let pdid = pdid.context("pdid not provided")?;

        let cq_id = cq_id.context("cq id not provided")?;
        let cq = Cq::new_cq(
            dma_buffer.subblock(CQ_PAGE * PAGE_SIZE, PAGE_SIZE),
            DoorbellPage::new(bar0.clone(), db_id)?,
            cq_id,
        );

        let rq_id = rq_id.context("rq id not provided")?;
        let rq = Wq::new_rq(
            dma_buffer.subblock(RQ_PAGE * PAGE_SIZE, PAGE_SIZE),
            DoorbellPage::new(bar0.clone(), db_id)?,
            rq_id,
        );

        let sq_id = sq_id.context("sq id not provided")?;
        let sq = Wq::new_sq(
            dma_buffer.subblock(SQ_PAGE * PAGE_SIZE, PAGE_SIZE),
            DoorbellPage::new(bar0.clone(), db_id)?,
            sq_id,
        );

        // To make debugging from the device side easier, randomize the upper
        // 16 bits of the ActivityId, so that requests can be distinguished.
        let mut rand_activity_id = [0_u8; 2];
        getrandom::fill(&mut rand_activity_id).unwrap();
        let hwc_activity_id = (u16::from_ne_bytes(rand_activity_id) as u32) << 16;
        let mut this = Self {
            device: Some(device),
            bar0,
            shmem_poll_timer: PolledTimer::new(driver),
            dma_buffer,
            eq,
            cq,
            rq,
            sq,
            interrupts: vec![Some(interrupt0)],
            test_events: 0,
            eq_armed,
            cq_armed: true,
            gpa_mkey,
            _pdid: pdid,
            eq_id_msix,
            num_msix,
            max_msix_available: num_msix,
            min_queue_avail: 0,
            hwc_activity_id,
            link_toggle: Vec::new(),
            hwc_subscribed: false,
            hwc_warning_time_in_ms: HWC_WARNING_TIME_IN_MS,
            hwc_timeout_in_ms: HWC_TIMEOUT_DEFAULT_IN_MS,
            hwc_failure: false,
            vtl2_interrupt_canary_supported: false,
            vtl2_interrupt_canary_reserved: false,
            state_saved: false,
            db_id,
            reset_request_pending: None,
        };

        this.push_rqe();

        let max_vf_resources = this
            .query_max_resources()
            .await
            .context("query_max_resources")?;
        tracing::info!("Max VF resources: {:?}", max_vf_resources);

        let device = this.device.as_mut().expect("device should be present");
        let max_msix_available = max_vf_resources.max_msix.min(device.max_interrupt_count());
        let num_msix = num_vps.min(max_msix_available);
        this.interrupts.resize_with(num_msix as usize, || None);
        this.num_msix = num_msix;
        this.max_msix_available = max_msix_available;
        this.min_queue_avail = max_vf_resources
            .max_eq
            .min(max_vf_resources.max_sq)
            .min(max_vf_resources.max_rq);

        Ok(this)
    }

    pub async fn save(&mut self) -> anyhow::Result<GdmaDriverSavedState> {
        if self.hwc_failure {
            anyhow::bail!("cannot save/restore after HWC failure");
        }

        if self.reset_request_pending.is_some() {
            anyhow::bail!("cannot save/restore with HWC reset request pending");
        }

        self.state_saved = true;

        let doorbell = self.bar0.save(Some(self.db_id as u64));

        Ok(GdmaDriverSavedState {
            mem: SavedMemoryState {
                base_pfn: self.dma_buffer.pfns()[0],
                len: self.dma_buffer.len(),
            },
            eq: self.eq.save(),
            cq: self.cq.save(),
            rq: self.rq.save(),
            sq: self.sq.save(),
            db_id: doorbell.doorbell_id,
            gpa_mkey: self.gpa_mkey,
            pdid: self._pdid,
            hwc_activity_id: self.hwc_activity_id,
            num_msix: self.num_msix,
            min_queue_avail: self.min_queue_avail,
            link_toggle: self.link_toggle.clone(),
            max_msix_available: self.max_msix_available,
        })
    }

    pub fn init(device: &mut T) -> anyhow::Result<(<T as DeviceBacking>::Registers, RegMap)> {
        let bar0_mapping = device.map_bar(0)?;
        let bar0_len = bar0_mapping.len();
        if bar0_len < size_of::<RegMap>() {
            anyhow::bail!("bar0 ({} bytes) too small for reg map", bar0_mapping.len());
        }

        let mut map = RegMap::new_zeroed();
        for i in 0..size_of_val(&map) / 4 {
            let v = bar0_mapping.read_u32(i * 4);
            // Unmapped device memory will return -1 on reads, so check the first 32
            // bits for this condition to get a clear error message early.
            if i == 0 && v == !0 {
                anyhow::bail!("bar0 read returned -1, device is not present");
            }
            map.as_mut_bytes()[i * 4..(i + 1) * 4].copy_from_slice(&v.to_ne_bytes());
        }

        tracing::debug!(?map, "register map");

        // Log on unknown major version numbers. This is not necessarily an
        // error, so continue.
        if map.major_version_number != 0 && map.major_version_number != 1 {
            tracing::warn!(
                major = map.major_version_number,
                minor = map.minor_version_number,
                micro = map.micro_version_number,
                "unrecognized major version"
            );
        }

        if map.vf_gdma_sriov_shared_sz != 32 {
            anyhow::bail!(
                "unexpected shared memory size: {}",
                map.vf_gdma_sriov_shared_sz
            );
        }

        if (bar0_len as u64).saturating_sub(map.vf_gdma_sriov_shared_reg_start)
            < map.vf_gdma_sriov_shared_sz as u64
        {
            anyhow::bail!(
                "bar0 ({} bytes) too small for shared memory at {}",
                bar0_mapping.len(),
                map.vf_gdma_sriov_shared_reg_start
            );
        }

        Ok((bar0_mapping, map))
    }

    pub async fn restore(
        driver: &impl Driver,
        saved_state: GdmaDriverSavedState,
        mut device: T,
        dma_buffer: MemoryBlock,
    ) -> anyhow::Result<Self> {
        tracing::info!("restoring gdma driver");

        let (bar0_mapping, map) = Self::init(&mut device)?;
        let doorbell_shift = map.vf_db_page_sz.trailing_zeros();

        let bar0 = Arc::new(Bar0 {
            mem: bar0_mapping,
            map,
            doorbell_shift,
        });

        let eq = Eq::restore_eq(
            dma_buffer.subblock(0, PAGE_SIZE),
            saved_state.eq,
            DoorbellPage::new(bar0.clone(), saved_state.db_id as u32)?,
        );

        let db_id = saved_state.db_id;
        let cq = Cq::restore_cq(
            dma_buffer.subblock(CQ_PAGE * PAGE_SIZE, PAGE_SIZE),
            saved_state.cq,
            DoorbellPage::new(bar0.clone(), saved_state.db_id as u32)?,
        );

        let rq = Wq::restore_rq(
            dma_buffer.subblock(RQ_PAGE * PAGE_SIZE, PAGE_SIZE),
            saved_state.rq,
            DoorbellPage::new(bar0.clone(), saved_state.db_id as u32)?,
        )?;

        let sq = Wq::restore_sq(
            dma_buffer.subblock(SQ_PAGE * PAGE_SIZE, PAGE_SIZE),
            saved_state.sq,
            DoorbellPage::new(bar0.clone(), saved_state.db_id as u32)?,
        )?;

        let mut interrupts = vec![None; saved_state.num_msix as usize];
        interrupts[0] = Some(device.map_interrupt(0, 0)?);
        let mut eq_id_msix = HashMap::new();
        eq_id_msix.insert(eq.id(), 0);

        let mut this = Self {
            device: Some(device),
            bar0,
            shmem_poll_timer: PolledTimer::new(driver),
            dma_buffer,
            interrupts,
            eq,
            cq,
            rq,
            sq,
            eq_id_msix,
            test_events: 0,
            eq_armed: true,
            cq_armed: true,
            gpa_mkey: saved_state.gpa_mkey,
            _pdid: saved_state.pdid,
            num_msix: saved_state.num_msix,
            max_msix_available: saved_state.max_msix_available.max(saved_state.num_msix),
            min_queue_avail: saved_state.min_queue_avail,
            hwc_activity_id: saved_state.hwc_activity_id,
            link_toggle: saved_state.link_toggle,
            hwc_subscribed: false,
            hwc_warning_time_in_ms: HWC_WARNING_TIME_IN_MS,
            hwc_timeout_in_ms: HWC_TIMEOUT_DEFAULT_IN_MS,
            hwc_failure: false,
            vtl2_interrupt_canary_supported: false,
            vtl2_interrupt_canary_reserved: false,
            state_saved: false,
            db_id: db_id as u32,
            reset_request_pending: None,
        };

        this.eq.arm();
        this.cq.arm();

        Ok(this)
    }

    async fn report_hwc_timeout(
        &mut self,
        last_cmd_failed: bool,
        interrupt_loss: bool,
        ms_elapsed: u32,
    ) {
        // Don't report timeout once HWC reset request is pending, SoC will not respond.
        if self.reset_request_pending.is_some() {
            return;
        }
        // Perform initial check for ownership, failing without wait if device
        // is not present or owns shmem region
        let data = self
            .bar0
            .mem
            .read_u32(self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 28);
        if data == u32::MAX {
            tracing::error!("Device no longer present");
            return;
        }
        let header = SmcProtoHdr::from(data);
        if header.owner_is_pf() {
            tracing::error!("pf owns shmem; skipping timeout report");
            return;
        }

        // Format and write payload information in the first seven 32-bit ranges
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize,
            self.rq.get_tail(),
        );
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 4,
            self.sq.get_tail(),
        );
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 8,
            self.cq.get_next(),
        );
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 12,
            self.eq.get_next(),
        );
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 16,
            0,
        );
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 20,
            0,
        );
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 24,
            ((last_cmd_failed as u32) << 24)
                | ((interrupt_loss as u32) << 25)
                | (ms_elapsed & 0xFFFFFF),
        );

        // Format and write header information in final 32-bit range, flipping
        // ownership to device for processing
        let msg_type = SmcMessageType::SMC_MSG_TYPE_REPORT_HWC_TIMEOUT.0;
        let hdr = SmcProtoHdr::new()
            .with_msg_type(msg_type)
            .with_msg_version(SMC_MSG_TYPE_REPORT_HWC_TIMEOUT_VERSION);
        let hdr = u32::from_le_bytes(hdr.as_bytes().try_into().expect("known size"));
        self.bar0.mem.write_u32(
            self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 28,
            hdr,
        );

        // Wait for the device to respond
        let max_wait_time =
            std::time::Instant::now() + Duration::from_millis(HWC_POLL_TIMEOUT_IN_MS);
        let header = loop {
            let data = self
                .bar0
                .mem
                .read_u32(self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 28);
            if data == u32::MAX {
                tracing::error!(msg_type, "device no longer present");
                return;
            }
            let header = SmcProtoHdr::from(data);
            if !header.owner_is_pf() {
                break header;
            }
            if std::time::Instant::now() > max_wait_time {
                tracing::error!(msg_type, "shmem wait for response (vf ownership) timed out");
                return;
            }
            std::hint::spin_loop();
        };
        if !header.is_response() {
            tracing::error!(msg_type, "expected shmem response");
        }
        if header.status() != 0 {
            tracing::error!(msg_type, header_status = header.status(), "response failed");
        }
    }

    pub fn get_link_toggle_list(&mut self) -> Vec<(u32, bool)> {
        self.link_toggle.split_off(0)
    }

    pub fn get_reset_request_pending(&self) -> Option<bool> {
        self.reset_request_pending
    }

    pub fn device(&self) -> &T {
        self.device.as_ref().unwrap()
    }

    pub fn check_vf_resources(&self, num_vps: u32, num_queues_needed: u32) {
        // Currently, the SoC and the MANA UMED caps the MSI-X/VF to 32,
        // independent of the number of vNICs configured.
        if self.num_msix < num_vps.min(num_queues_needed) {
            tracing::warn!(
                num_queues_needed,
                self.num_msix,
                "Not enough MSI-X available to deliver required MANA network performance"
            )
        }

        let queue_avail = self
            .min_queue_avail
            .saturating_sub(self.vtl2_interrupt_canary_reserved as u32);
        if num_queues_needed > queue_avail {
            tracing::error!(
                num_queues_needed,
                queue_avail,
                "Not enough EQ's available to support all vNICs"
            )
        }
    }

    fn push_rqe(&mut self) {
        let n = self
            .rq
            .push(
                (),
                [Sge {
                    address: self.dma_buffer.pfns()[RESPONSE_PAGE] * PAGE_SIZE64,
                    mem_key: self.gpa_mkey,
                    size: PAGE_SIZE as u32,
                }],
            )
            .expect("rq is not full");
        assert_eq!(n, RWQE_SIZE);
        self.rq.commit();
    }

    pub async fn request_version<
        Req: IntoBytes + Immutable + KnownLayout,
        Resp: IntoBytes + FromBytes + Immutable + KnownLayout,
    >(
        &mut self,
        req_msg_type: u32,
        req_msg_version: u16,
        resp_msg_type: u32,
        resp_msg_version: u16,
        dev_id: GdmaDevId,
        req: Req,
    ) -> anyhow::Result<(Resp, u32)> {
        if self.reset_request_pending.is_some() {
            anyhow::bail!("HWC reset request pending");
        }
        if self.hwc_failure {
            anyhow::bail!("Previous hardware failure");
        }
        let req_hdr = GdmaMsgHdr {
            hdr_type: GDMA_STANDARD_HEADER_TYPE,
            msg_type: req_msg_type,
            msg_version: req_msg_version,
            hwc_msg_id: 0,
            msg_size: (size_of::<GdmaReqHdr>() + size_of_val(&req)) as u32,
        };
        let expected_resp_hdr = GdmaMsgHdr {
            msg_type: resp_msg_type,
            msg_version: resp_msg_version,
            msg_size: (size_of::<GdmaRespHdr>() + size_of::<Resp>()) as u32,
            ..req_hdr
        };
        self.hwc_activity_id = self.hwc_activity_id.wrapping_add(1);
        let hdr = GdmaReqHdr {
            req: req_hdr,
            resp: expected_resp_hdr,
            dev_id,
            activity_id: self.hwc_activity_id,
        };

        tracing::trace!(
            request = format!("{:#x}", req_msg_type),
            activity_id = format!("{:#x}", hdr.activity_id),
            "HWC request",
        );
        // Zero the response page for the expected response size before sending
        // the request. This ensures that fields added in newer response versions
        // read as zero when talking to an older socmana that does not populate
        // them, rather than containing stale data.
        let expected_resp_size = size_of::<GdmaRespHdr>() + size_of::<Resp>();
        assert!(
            expected_resp_size <= PAGE_SIZE,
            "response size {expected_resp_size} exceeds {PAGE_SIZE}"
        );
        self.dma_buffer
            .write_zeros(RESPONSE_PAGE * PAGE_SIZE, expected_resp_size);

        self.dma_buffer.write_obj(REQUEST_PAGE * PAGE_SIZE, &hdr);
        self.dma_buffer
            .write_obj(REQUEST_PAGE * PAGE_SIZE + size_of_val(&hdr), &req);

        let oob = HwcTxOob {
            flags3: HwcTxOobFlags3::new().with_vscq_id(self.cq.id()),
            flags4: HwcTxOobFlags4::new().with_vsq_id(self.sq.id()),
            ..FromZeros::new_zeroed()
        };

        let hw_access = async {
            let sqe_len = self
                .sq
                .push(
                    oob,
                    [Sge {
                        address: self.dma_buffer.pfns()[REQUEST_PAGE] * PAGE_SIZE64,
                        mem_key: self.gpa_mkey,
                        size: (size_of_val(&hdr) + size_of_val(&req)) as u32,
                    }],
                )
                .expect("send queue should not be full");

            self.sq.commit();
            let req_phys_addr = self.dma_buffer.pfns()[REQUEST_PAGE] * PAGE_SIZE64;
            let sgl_phys_addr = self.dma_buffer.pfns()[SQ_PAGE] * PAGE_SIZE64;
            let mem_key = self.gpa_mkey;
            let cq_wait_context = || {
                format!(
                    "HWC request failed. request={:#x}, activity_id={:#x}, queue_phys_addr={:#x}, req_phys_addr={:#x}, write_size={}, mem_key={:#x}",
                    req_msg_type,
                    hdr.activity_id,
                    sgl_phys_addr,
                    req_phys_addr,
                    size_of_val(&hdr) + size_of_val(&req),
                    mem_key,
                )
            };
            self.wait_cq().await.with_context(cq_wait_context)?;
            self.wait_cq().await.with_context(cq_wait_context)?;
            self.sq.advance_head(sqe_len);
            self.rq.advance_head(RWQE_SIZE);
            self.push_rqe();

            let resp_hdr = self
                .dma_buffer
                .read_obj::<GdmaRespHdr>(RESPONSE_PAGE * PAGE_SIZE);

            if resp_hdr.response.msg_size < size_of::<Resp>() as u32 {
                anyhow::bail!(
                    "response too small, request={:#x}, activity_id={:#x}",
                    req_msg_type,
                    hdr.activity_id
                );
            }
            if resp_hdr.status != 0 {
                anyhow::bail!(
                    "failed with {:#x}, request={:#x}, activity_id={:#x}",
                    resp_hdr.status,
                    req_msg_type,
                    hdr.activity_id
                );
            }

            let resp = self
                .dma_buffer
                .read_obj::<Resp>(RESPONSE_PAGE * PAGE_SIZE + size_of_val(&resp_hdr));
            Ok(resp)
        };
        let resp = match hw_access.await {
            Ok(resp) => resp,
            Err(err) => {
                self.hwc_failure = true;
                return Err(err);
            }
        };

        tracing::trace!(
            request = format!("{:#x}", req_msg_type),
            activity_id = format!("{:#x}", hdr.activity_id),
            "HWC response success",
        );
        Ok((resp, self.hwc_activity_id))
    }

    pub async fn request<
        Req: IntoBytes + Immutable + KnownLayout,
        Resp: IntoBytes + FromBytes + Immutable + KnownLayout,
    >(
        &mut self,
        msg_type: u32,
        dev_id: GdmaDevId,
        req: Req,
    ) -> anyhow::Result<Resp> {
        let (resp, _) = self
            .request_version(
                msg_type,
                GDMA_MESSAGE_V1,
                msg_type,
                GDMA_MESSAGE_V1,
                dev_id,
                req,
            )
            .await?;

        Ok(resp)
    }

    pub fn hwc_subscribe(&mut self) -> DeviceInterrupt {
        let interrupt = self.interrupts[0].clone().unwrap();
        if !self.eq_armed {
            self.eq.arm();
            self.eq_armed = true;
        }
        self.hwc_subscribed = true;
        interrupt
    }

    pub fn process_all_eqs(&mut self) -> bool {
        let mut eqe_found = false;
        while let Some(eqe) = self.eq.pop() {
            self.eq_armed = false;
            eqe_found = true;
            match eqe.params.event_type() {
                GDMA_EQE_COMPLETION => self.cq_armed = false,
                GDMA_EQE_TEST_EVENT => self.test_events += 1,
                GDMA_EQE_HWC_RECONFIG_DATA => {
                    let data = EqeDataReconfig::read_from_prefix(&eqe.data[..]).unwrap().0; // TODO: zerocopy: use-rest-of-range (https://github.com/microsoft/openvmm/issues/759)
                    let mut value: [u8; 4] = [0; 4];
                    value[0..3].copy_from_slice(&data.data);
                    let value: u32 = u32::from_le_bytes(value);
                    match data.data_type {
                        HWC_DATA_TYPE_HW_VPORT_LINK_CONNECT
                        | HWC_DATA_TYPE_HW_VPORT_LINK_DISCONNECT => {
                            let link_connect =
                                data.data_type == HWC_DATA_TYPE_HW_VPORT_LINK_CONNECT;
                            self.link_toggle.push((value, link_connect));
                            tracing::trace!(value, link_connect, "link status: vport index");
                        }
                        HWC_DATA_CONFIG_HWC_TIMEOUT => {
                            self.hwc_timeout_in_ms = value;
                            tracing::info!(
                                hwc_timeout_in_ms = self.hwc_timeout_in_ms,
                                "HWC timeout value"
                            );
                        }
                        unknown => tracing::error!(unknown, "unknown reconfig data type"),
                    }
                }
                GDMA_EQE_HWC_RESET_REQUEST => {
                    let data = EqeVfReset::read_from_prefix(&eqe.data[..]).unwrap().0;
                    let revoke_vtl0_vf = data.revoke_vtl0_vf();
                    tracing::info!(revoke_vtl0_vf, "HWC VF reset request");
                    self.reset_request_pending = Some(revoke_vtl0_vf);
                }
                ty => tracing::error!(ty, "unknown eq event"),
            }
            self.eq.ack();
        }

        if !self.eq_armed && self.hwc_subscribed {
            self.eq.arm();
            self.eq_armed = true;
        }
        eqe_found
    }

    async fn wait_for_hwc_interrupt(
        hwc_event: &mut DeviceInterrupt,
        hwc_failure: Option<&mut bool>,
        hwc_timeout_in_ms: u32,
    ) -> anyhow::Result<()> {
        let mut ctx = mesh::CancelContext::new()
            .with_timeout(Duration::from_millis(hwc_timeout_in_ms as u64));
        if let Err(err) = ctx.until_cancelled(hwc_event.wait()).await {
            if let Some(failed) = hwc_failure {
                *failed = true;
            }
            return Err(err).context("MANA request timed out. Waiting for HWC interrupt.");
        };

        Ok(())
    }

    async fn process_eqs_or_wait_with_retry(&mut self) -> EqeWaitResult {
        let mut eqe_wait_result = EqeWaitResult {
            eqe_found: false,
            elapsed: 0,
            eq_arm_count: 0,
            interrupt_wait_count: 0,
            interrupt_count: 0,
            last_wait_result: Ok(()),
        };
        loop {
            // Arm the EQ if it is not already armed.
            if !self.eq_armed {
                eqe_wait_result.eq_arm_count += 1;
                self.eq.arm();
                self.eq_armed = true;
                // Check if the event arrived while arming.
                if self.process_all_eqs() {
                    // Remove any pending interrupt events.
                    let _ = self.interrupts[0].as_mut().unwrap().wait().now_or_never();
                    eqe_wait_result.eqe_found = true;
                    eqe_wait_result.last_wait_result = Ok(()); // Reset last_wait_result.
                    break eqe_wait_result;
                }
            }

            // Wait for an interrupt.
            eqe_wait_result.interrupt_wait_count += 1;
            let ms_wait = (HWC_INTERRUPT_POLL_WAIT_MIN_MS
                * 2u32.pow(eqe_wait_result.interrupt_wait_count - 1))
            .min(HWC_INTERRUPT_POLL_WAIT_MAX_MS)
            .min(
                self.hwc_timeout_in_ms
                    .saturating_sub(eqe_wait_result.elapsed as u32),
            );
            let before_wait = std::time::Instant::now();
            eqe_wait_result.last_wait_result = Self::wait_for_hwc_interrupt(
                self.interrupts[0].as_mut().unwrap(),
                Some(&mut self.hwc_failure),
                ms_wait,
            )
            .await;
            eqe_wait_result.elapsed += before_wait.elapsed().as_millis();
            if eqe_wait_result.last_wait_result.is_ok() {
                eqe_wait_result.interrupt_count += 1;
            }

            // Poll for EQ events.
            if self.process_all_eqs() {
                eqe_wait_result.eqe_found = true;
                break eqe_wait_result;
            }

            // Exit with no eqe found if timeout occurs.
            if eqe_wait_result.elapsed >= self.hwc_timeout_in_ms as u128 {
                eqe_wait_result.eqe_found = false;
                break eqe_wait_result;
            }
        }
    }

    async fn process_eqs_or_wait(&mut self) -> anyhow::Result<()> {
        let eqe_wait_result = self.process_eqs_or_wait_with_retry().await;
        let wait_failed = !eqe_wait_result.eqe_found;
        let interrupt_loss = eqe_wait_result.interrupt_wait_count != 0
            && eqe_wait_result.interrupt_count == 0
            && !wait_failed;
        if wait_failed
            || eqe_wait_result.elapsed > self.hwc_warning_time_in_ms as u128
            || interrupt_loss
        {
            tracing::warn!(
                wait_failed,
                wait_ms = eqe_wait_result.elapsed,
                int_loss = interrupt_loss,
                int_count = eqe_wait_result.interrupt_count,
                int_waits = eqe_wait_result.interrupt_wait_count,
                arm_count = eqe_wait_result.eq_arm_count,
                warn_ms = self.hwc_warning_time_in_ms,
                "hwc {}",
                match (wait_failed, interrupt_loss) {
                    (true, _) => "timeout waiting for response",
                    (_, true) =>
                        "response received with interrupt wait attempted but no interrupt received",
                    _ => "response received with delay",
                }
            );
            self.report_hwc_timeout(wait_failed, interrupt_loss, eqe_wait_result.elapsed as u32)
                .await;
            if !wait_failed && eqe_wait_result.elapsed > self.hwc_warning_time_in_ms as u128 {
                // Increase warning threshold after each delay warning occurrence.
                self.hwc_warning_time_in_ms += HWC_WARNING_INCREASE_IN_MS;
            }
        } else if eqe_wait_result.interrupt_wait_count != 0 || eqe_wait_result.eq_arm_count != 0 {
            tracing::trace!(
                wait_ms = eqe_wait_result.elapsed,
                int_count = eqe_wait_result.interrupt_count,
                int_waits = eqe_wait_result.interrupt_wait_count,
                arm_count = eqe_wait_result.eq_arm_count,
                "found HWC response EQE after arm or wait",
            );
        }
        if wait_failed {
            self.hwc_failure = true;
            if eqe_wait_result.last_wait_result.is_err() {
                return eqe_wait_result.last_wait_result;
            } else {
                return Err(anyhow::anyhow!(
                    "MANA request timed out. No EQE found for HWC response."
                ));
            }
        }
        self.hwc_failure = false;
        Ok(())
    }

    async fn wait_cq(&mut self) -> anyhow::Result<Cqe> {
        loop {
            if let Some(cqe) = self.cq.pop() {
                self.cq_armed = false;
                return Ok(cqe);
            }
            if !self.cq_armed {
                self.cq.arm();
                self.cq_armed = true;
                // Check if the event arrived while arming.
                if let Some(cqe) = self.cq.pop() {
                    // Consume any EQ events.
                    self.process_all_eqs();
                    self.cq_armed = false;
                    // Remove any pending interrupt events.
                    let _ = self.interrupts[0].as_mut().unwrap().wait().now_or_never();
                    return Ok(cqe);
                }
            }
            self.process_eqs_or_wait().await?;
        }
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub async fn test_eq(&mut self) -> anyhow::Result<()> {
        let n = self.test_events;
        self.request::<_, ()>(
            GdmaRequestType::GDMA_GENERATE_TEST_EQE.0,
            HWC_DEV_ID,
            GdmaGenerateTestEventReq {
                queue_index: self.eq.id(),
            },
        )
        .await?;
        while self.test_events == n {
            self.process_eqs_or_wait().await.with_context(|| {
                format!(
                    "HWC request failed. request={:#x}, activity_id={:#x}",
                    GdmaRequestType::GDMA_GENERATE_TEST_EQE.0,
                    self.hwc_activity_id
                )
            })?;
        }
        Ok(())
    }

    #[cfg(test)]
    #[tracing::instrument(skip(self), level = "debug", err)]
    pub async fn generate_reset_request_eqe(&mut self, revoke_vtl0_vf: bool) -> anyhow::Result<()> {
        let reset = EqeVfReset::new().with_revoke_vtl0_vf(revoke_vtl0_vf);
        self.request::<_, ()>(
            GdmaRequestType::GDMA_GENERATE_RESET_REQUEST_EQE.0,
            HWC_DEV_ID,
            GdmaGenerateResetEventReq {
                queue_index: self.eq.id(),
                data: reset,
            },
        )
        .await?;
        Ok(())
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub async fn verify_vf_driver_version(&mut self) -> anyhow::Result<()> {
        let ver = &build_info::OPENHCL_VERSION;

        let mut req = GdmaVerifyVerReq {
            protocol_ver_min: 1,
            protocol_ver_max: 1,
            gd_drv_cap_flags1: DRIVER_CAP_FLAG_1_VARIABLE_INDIRECTION_TABLE_SUPPORT
                | DRIVER_CAP_FLAG_1_HW_VPORT_LINK_AWARE
                | DRIVER_CAP_FLAG_1_HWC_TIMEOUT_RECONFIG
                | DRIVER_CAP_FLAG_1_SELF_RESET_ON_EQE_NOTIFICATION
                | DRIVER_CAP_FLAG_1_VTL2_REVOKE_SUB_ON_RESET_EQE
                | DRIVER_CAP_FLAG_1_VTL2_SELECTIVE_REVOKE_SUB_ON_RESET_EQE
                | DRIVER_CAP_FLAG_1_VTL2_INTERRUPT_CANARY,
            os_type: gdma_defs::OS_TYPE_OHCL,
            os_ver_major: ver.major(),
            os_ver_minor: ver.minor(),
            os_ver_build: ver.build(),
            os_ver_platform: ver.platform(),
            ..FromZeros::new_zeroed()
        };

        // Identify the driver and build to the SOC
        // str1 = "OpenHCL", str2 = build identity.
        let name = ver.product_name().as_bytes();
        let len = name.len().min(req.os_ver_str1.len().saturating_sub(1));
        req.os_ver_str1[..len].copy_from_slice(&name[..len]);

        let revision = build_info::get().scm_revision().as_bytes();
        let len = revision.len().min(req.os_ver_str2.len().saturating_sub(1));
        req.os_ver_str2[..len].copy_from_slice(&revision[..len]);

        let resp: GdmaVerifyVerResp = self
            .request(
                GdmaRequestType::GDMA_VERIFY_VF_DRIVER_VERSION.0,
                HWC_DEV_ID,
                req,
            )
            .await?;

        if resp.gdma_protocol_ver != 1 {
            anyhow::bail!("invalid protocol version");
        }

        self.vtl2_interrupt_canary_supported =
            (resp.pf_cap_flags2 & GDMA_PF_CAP_FLAG_2_VTL2_INTERRUPT_CANARY) != 0;

        tracing::info!(
            gdma_protocol_ver = resp.gdma_protocol_ver,
            pf_cap_flags1 = format_args!("{:#x}", resp.pf_cap_flags1),
            pf_cap_flags2 = format_args!("{:#x}", resp.pf_cap_flags2),
            pf_cap_flags3 = format_args!("{:#x}", resp.pf_cap_flags3),
            pf_cap_flags4 = format_args!("{:#x}", resp.pf_cap_flags4),
            vtl2_interrupt_canary_supported = self.vtl2_interrupt_canary_supported,
            "GDMA PF capability flags",
        );

        Ok(())
    }

    pub async fn query_max_resources(&mut self) -> anyhow::Result<GdmaQueryMaxResourcesResp> {
        self.request(GdmaRequestType::GDMA_QUERY_MAX_RESOURCES.0, HWC_DEV_ID, ())
            .await
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub async fn list_devices(&mut self) -> anyhow::Result<Vec<GdmaDevId>> {
        let resp: GdmaListDevicesResp = self
            .request(GdmaRequestType::GDMA_LIST_DEVICES.0, HWC_DEV_ID, ())
            .await?;
        Ok(resp.devs[..resp.num_of_devs as usize].to_vec())
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub async fn register_device(
        &mut self,
        dev_id: GdmaDevId,
    ) -> anyhow::Result<GdmaRegisterDeviceResp> {
        self.request(GdmaRequestType::GDMA_REGISTER_DEVICE.0, dev_id, ())
            .await
    }

    pub async fn deregister_device(&mut self, dev_id: GdmaDevId) -> anyhow::Result<()> {
        self.hwc_timeout_in_ms = HWC_TIMEOUT_FOR_SHUTDOWN_IN_MS;
        self.request(GdmaRequestType::GDMA_DEREGISTER_DEVICE.0, dev_id, ())
            .await
    }

    pub fn into_device(mut self) -> T {
        self.device.take().unwrap()
    }

    fn start_listening(&mut self, eq_id: u32, msix: u32) -> DeviceInterrupt {
        let interrupt = self.interrupts[msix as usize]
            .clone()
            .expect("MSI-X should be present");
        if self.eq_id_msix.insert(eq_id, msix).is_some() {
            panic!(
                "duplicate eq id {}, [id, msix] {:?}",
                eq_id, self.eq_id_msix
            );
        }
        interrupt
    }

    fn stop_listening(&mut self, eq_id: u32) {
        self.eq_id_msix.remove(&eq_id);
    }

    fn map_msix(&mut self, msix: u32, cpu: u32) -> anyhow::Result<()> {
        let device = self.device.as_mut().expect("device should be present");
        let interrupt = device.map_interrupt(msix, cpu)?;
        if self.interrupts.len() <= msix as usize {
            self.interrupts.resize_with(msix as usize + 1, || None);
        }
        self.interrupts[msix as usize] = Some(interrupt);
        Ok(())
    }

    fn get_msix_for_cpu(&mut self, cpu: u32) -> anyhow::Result<u32> {
        let msix = cpu % self.num_msix;
        self.map_msix(msix, cpu)?;
        Ok(msix)
    }

    async fn create_eq_with_msix(
        &mut self,
        arena: &mut ResourceArena,
        dev_id: GdmaDevId,
        gdma_region: u64,
        queue_size: u32,
        pdid: u32,
        doorbell_id: u32,
        msix: u32,
    ) -> anyhow::Result<(u32, DeviceInterrupt)> {
        let resp: GdmaCreateQueueResp = self
            .request(
                GdmaRequestType::GDMA_CREATE_QUEUE.0,
                dev_id,
                GdmaCreateQueueReq {
                    queue_type: GdmaQueueType::GDMA_EQ,
                    pdid,
                    doorbell_id,
                    gdma_region,
                    queue_size,
                    eq_pci_msix_index: msix,
                    ..FromZeros::new_zeroed()
                },
            )
            .await?;

        // The EQ takes ownership of the DMA region.
        arena.take_dma_region(gdma_region);
        arena.push(Resource::Eq {
            dev_id,
            eq_id: resp.queue_index,
        });
        let interrupt = self.start_listening(resp.queue_index, msix);
        Ok((resp.queue_index, interrupt))
    }

    pub(crate) async fn initialize_interrupt_canary(
        &mut self,
        saved_state: Option<&InterruptCanarySavedState>,
    ) -> anyhow::Result<Option<InterruptCanary>> {
        if saved_state.is_none() && !self.vtl2_interrupt_canary_supported {
            return Ok(None);
        }

        if self.dma_buffer.len() < (VTL2_INTERRUPT_CANARY_EQ_PAGE + 1) * PAGE_SIZE {
            tracing::warn!(
                dma_buffer_len = self.dma_buffer.len(),
                "VTL2 interrupt canary DMA page is unavailable"
            );
            return Ok(None);
        }

        let eq_mem = self
            .dma_buffer
            .subblock(VTL2_INTERRUPT_CANARY_EQ_PAGE * PAGE_SIZE, PAGE_SIZE);

        if let Some(saved_state) = saved_state {
            let max_interrupt_count = self
                .device
                .as_ref()
                .expect("device should be present")
                .max_interrupt_count();
            if saved_state.msix >= max_interrupt_count {
                anyhow::bail!(
                    "saved interrupt canary MSI-X {} exceeds device limit {}",
                    saved_state.msix,
                    max_interrupt_count
                );
            }

            self.map_msix(saved_state.msix, 0)?;
            let eq = Eq::restore_eq(
                eq_mem.clone(),
                saved_state.eq.clone(),
                DoorbellPage::new(self.bar0.clone(), self.db_id)?,
            );
            let interrupt = self.start_listening(eq.id(), saved_state.msix);
            let resources = ResourceArena::restore_eq(eq_mem, HWC_DEV_ID, eq.id());
            self.vtl2_interrupt_canary_reserved = true;
            return Ok(Some(InterruptCanary::restore(
                eq,
                interrupt,
                resources,
                saved_state.msix,
            )));
        }

        if self.num_msix == 0
            || self.max_msix_available <= self.num_msix
            || self.min_queue_avail == 0
        {
            tracing::warn!(
                data_msix = self.num_msix,
                max_msix_available = self.max_msix_available,
                min_queue_avail = self.min_queue_avail,
                "VTL2 interrupt canary requires one spare EQ and MSI-X vector"
            );
            return Ok(None);
        }

        let msix = self.num_msix;
        self.map_msix(msix, 0)?;

        let mut resources = ResourceArena::new();
        let gdma_region = self
            .create_dma_region(&mut resources, HWC_DEV_ID, eq_mem.clone())
            .await?;
        let (eq_id, interrupt) = match self
            .create_eq_with_msix(
                &mut resources,
                HWC_DEV_ID,
                gdma_region,
                PAGE_SIZE as u32,
                self._pdid,
                self.db_id,
                msix,
            )
            .await
        {
            Ok(value) => value,
            Err(err) => {
                resources.destroy(self).await;
                return Err(err);
            }
        };

        let eq = Eq::new_eq(
            eq_mem,
            DoorbellPage::new(self.bar0.clone(), self.db_id)?,
            eq_id,
        );
        self.vtl2_interrupt_canary_reserved = true;
        Ok(Some(InterruptCanary::new(eq, interrupt, resources, msix)))
    }

    pub(crate) async fn configure_interrupt_canary(
        &mut self,
        request: GdmaConfigureVtl2InterruptCanaryReq,
    ) -> anyhow::Result<()> {
        if !self.vtl2_interrupt_canary_supported {
            if request.enable == 0 {
                return Ok(());
            }
            anyhow::bail!("VTL2 interrupt canary is not supported by the PF");
        }

        self.request(
            GdmaRequestType::GDMA_CONFIGURE_VTL2_INTERRUPT_CANARY.0,
            HWC_DEV_ID,
            request,
        )
        .await
    }

    pub(crate) fn interrupt_canary_supported(&self) -> bool {
        self.vtl2_interrupt_canary_supported
    }

    pub(crate) fn mark_hwc_failure(&mut self) {
        self.hwc_failure = true;
    }

    async fn wait_for_interrupt_canary_shmem(&mut self) -> Option<SmcProtoHdr> {
        let shmem_header = self.bar0.map.vf_gdma_sriov_shared_reg_start as usize + 28;
        let deadline = std::time::Instant::now()
            + Duration::from_millis(VTL2_INTERRUPT_CANARY_SHMEM_TIMEOUT_MS);

        loop {
            let value = self.bar0.mem.read_u32(shmem_header);
            if value == u32::MAX {
                tracing::error!("device no longer present while reporting interrupt canary");
                return None;
            }

            let header = SmcProtoHdr::from(value);
            if !header.owner_is_pf() {
                return Some(header);
            }
            if std::time::Instant::now() >= deadline {
                tracing::error!("timed out waiting for interrupt canary SHMEM ownership");
                return None;
            }
            self.shmem_poll_timer
                .sleep(Duration::from_millis(VTL2_INTERRUPT_CANARY_SHMEM_POLL_MS))
                .await;
        }
    }

    pub(crate) async fn report_interrupt_canary(&mut self, report: InterruptCanaryReport) -> bool {
        if self.reset_request_pending.is_some() || !self.vtl2_interrupt_canary_supported {
            return false;
        }

        let shmem_base = self.bar0.map.vf_gdma_sriov_shared_reg_start as usize;
        if self.wait_for_interrupt_canary_shmem().await.is_none() {
            return false;
        }

        let result = match report.failure {
            InterruptCanaryFailure::EqePendingNoInterrupt => {
                SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_EQE_PENDING_NO_INTERRUPT
            }
            InterruptCanaryFailure::EqePendingAfterInterrupt => {
                SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_EQE_PENDING_AFTER_INTERRUPT
            }
            InterruptCanaryFailure::InterruptWithoutEqe => {
                SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_INTERRUPT_NO_EQE
            }
            InterruptCanaryFailure::UnexpectedEqe => {
                SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_UNEXPECTED_EQE
            }
            InterruptCanaryFailure::SequenceMismatch => {
                SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_SEQUENCE_MISMATCH
            }
        };
        let params = SMC_GDMA_VTL2_INTERRUPT_CANARY_VALID
            | SMC_GDMA_VTL2_INTERRUPT_CANARY_LEGACY_PROBE
            | (result << SMC_GDMA_VTL2_INTERRUPT_CANARY_RESULT_SHIFT)
            | report
                .pending_ms
                .min(SMC_GDMA_VTL2_INTERRUPT_CANARY_PENDING_MS_MASK);
        let payload = [
            report.generation,
            report.expected_sequence,
            report.observed_sequence,
            report.queue_id,
            report.eq_next,
            report.interrupt_count,
            params,
        ];
        for (index, value) in payload.into_iter().enumerate() {
            self.bar0.mem.write_u32(shmem_base + index * 4, value);
        }
        safe_intrinsics::store_fence();
        let header = SmcProtoHdr::new()
            .with_msg_type(SmcMessageType::SMC_MSG_TYPE_REPORT_HWC_TIMEOUT.0)
            .with_msg_version(SMC_MSG_TYPE_REPORT_VTL2_INTERRUPT_CANARY_VERSION);
        self.bar0.mem.write_u32(
            shmem_base + 28,
            u32::from_le_bytes(header.as_bytes().try_into().expect("known size")),
        );

        let Some(response) = self.wait_for_interrupt_canary_shmem().await else {
            return false;
        };
        if !response.is_response() || response.status() != 0 {
            tracing::error!(
                generation = report.generation,
                expected_sequence = report.expected_sequence,
                observed_sequence = report.observed_sequence,
                is_response = response.is_response(),
                status = response.status(),
                "VTL2 interrupt canary SHMEM report failed"
            );
            return false;
        }

        tracing::error!(
            generation = report.generation,
            expected_sequence = report.expected_sequence,
            observed_sequence = report.observed_sequence,
            queue_id = report.queue_id,
            eq_next = report.eq_next,
            interrupt_count = report.interrupt_count,
            poll_count = report.poll_count,
            pending_ms = report.pending_ms,
            failure = ?report.failure,
            "reported VTL2 interrupt canary failure"
        );
        true
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub async fn retarget_eq(
        &mut self,
        dev_id: GdmaDevId,
        eq_id: u32,
        cpu: u32,
    ) -> anyhow::Result<Option<DeviceInterrupt>> {
        let msix_to = self.get_msix_for_cpu(cpu)?;
        tracing::info!("retargeting EQ {} to cpu: {}", eq_id, cpu);
        if let Some(msix) = self.eq_id_msix.get(&eq_id) {
            if *msix == msix_to {
                tracing::trace!("eq is already mapped to this msix, skipping");
                return Ok(None);
            }
        }
        self.stop_listening(eq_id);
        self.request::<_, ()>(
            GdmaRequestType::GDMA_CHANGE_MSIX_FOR_EQ.0,
            dev_id,
            GdmaChangeMsixVectorIndexForEq {
                queue_index: eq_id,
                msix: msix_to,
                reserved1: 0,
                reserved2: 0,
            },
        )
        .await?;
        let interrupt = self.start_listening(eq_id, msix_to);
        Ok(Some(interrupt))
    }

    #[tracing::instrument(skip(self, arena), level = "debug", err)]
    pub async fn create_eq(
        &mut self,
        arena: &mut ResourceArena,
        dev_id: GdmaDevId,
        gdma_region: u64,
        queue_size: u32,
        pdid: u32,
        doorbell_id: u32,
        cpu: u32,
    ) -> anyhow::Result<(u32, DeviceInterrupt)> {
        let msix = self.get_msix_for_cpu(cpu)?;
        let result = self
            .create_eq_with_msix(
                arena,
                dev_id,
                gdma_region,
                queue_size,
                pdid,
                doorbell_id,
                msix,
            )
            .await?;
        tracing::trace!(id = result.0, cpu, msix, "created eq",);
        Ok(result)
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub(crate) async fn disable_eq(&mut self, dev_id: GdmaDevId, eq_id: u32) -> anyhow::Result<()> {
        self.stop_listening(eq_id);
        self.request(
            GdmaRequestType::GDMA_DISABLE_QUEUE.0,
            dev_id,
            GdmaDisableQueueReq {
                queue_type: GdmaQueueType::GDMA_EQ,
                queue_index: eq_id,
                alloc_res_id_on_creation: 1, /* what is this? */
            },
        )
        .await
    }

    #[tracing::instrument(skip_all, level = "debug", err)]
    pub async fn create_dma_region(
        &mut self,
        arena: &mut ResourceArena,
        dev_id: GdmaDevId,
        mem: MemoryBlock,
    ) -> anyhow::Result<u64> {
        #[repr(C)]
        #[derive(IntoBytes, Immutable, KnownLayout)]
        struct Req {
            req: GdmaCreateDmaRegionReq,
            pages: [u64; 16],
        }
        let pages = mem.pfns();
        let mut req = Req {
            req: GdmaCreateDmaRegionReq {
                length: mem.len() as u64,
                offset_in_page: mem.offset_in_page(),
                gdma_page_type: GDMA_PAGE_TYPE_4K,
                page_count: pages.len() as u32,
                page_addr_list_len: pages.len() as u32,
            },
            pages: [0; 16],
        };
        for (d, &s) in req.pages[..pages.len()].iter_mut().zip(pages) {
            *d = s * PAGE_SIZE64;
        }
        let resp: GdmaCreateDmaRegionResp = self
            .request(GdmaRequestType::GDMA_CREATE_DMA_REGION.0, dev_id, req)
            .await?;

        arena.push(Resource::MemoryBlock(ManuallyDrop::new(mem)));
        arena.push(Resource::DmaRegion {
            dev_id,
            gdma_region: resp.gdma_region,
        });

        // TODO: AddPages for larger region
        Ok(resp.gdma_region)
    }

    #[tracing::instrument(skip(self), level = "debug", err)]
    pub(crate) async fn destroy_dma_region(
        &mut self,
        dev_id: GdmaDevId,
        gdma_region: u64,
    ) -> anyhow::Result<()> {
        self.request(
            GdmaRequestType::GDMA_DESTROY_DMA_REGION.0,
            dev_id,
            GdmaDestroyDmaRegionReq { gdma_region },
        )
        .await
    }
}

#[cfg(test)]
mod interrupt_canary_tests {
    use super::*;
    use gdma_defs::Eqe;
    use gdma_defs::EqeParams;
    use user_driver::DmaClient;
    use user_driver::interrupt::DeviceInterruptSource;
    use user_driver_emulated_mock::DeviceTestMemory;

    fn new_test_canary() -> (InterruptCanary, MemoryBlock, DeviceInterruptSource) {
        let mem = DeviceTestMemory::new(8, false, "interrupt_canary_recovery");
        let dma_client = mem.dma_client();
        let eq_mem = dma_client.allocate_dma_buffer(PAGE_SIZE).unwrap();
        let interrupt_source = DeviceInterruptSource::new();
        let interrupt = interrupt_source.new_target();
        let canary = InterruptCanary::new(
            Eq::new_eq(eq_mem.clone(), DoorbellPage::null(), 7),
            interrupt,
            ResourceArena::new(),
            4,
        );
        (canary, eq_mem, interrupt_source)
    }

    fn post_test_eqe(eq_mem: &MemoryBlock, offset: usize, sequence: u32) {
        let mut data = [0; 12];
        data[..4].copy_from_slice(&sequence.to_le_bytes());
        eq_mem.write_obj(
            offset,
            &Eqe {
                data,
                params: EqeParams::new()
                    .with_event_type(GDMA_EQE_TEST_EVENT)
                    .with_owner_count(1),
            },
        );
    }

    fn pending_report(canary: &InterruptCanary) -> InterruptCanaryReport {
        canary.make_report(
            canary.expected_sequence,
            VTL2_INTERRUPT_CANARY_PENDING_POLLS * VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
            InterruptCanaryFailure::EqePendingNoInterrupt,
        )
    }

    #[test]
    fn pending_eqe_is_reported_once() {
        let mut watchdog = InterruptCanaryWatchdog::default();
        let event = Some((GDMA_EQE_TEST_EVENT, 7));

        for _ in 1..VTL2_INTERRUPT_CANARY_PENDING_POLLS {
            assert_eq!(watchdog.observe(event, 0, 0), None);
        }
        assert_eq!(
            watchdog.observe(event, 0, 0),
            Some((
                InterruptCanaryFailure::EqePendingNoInterrupt,
                VTL2_INTERRUPT_CANARY_PENDING_POLLS * VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
                0,
            ))
        );
        assert_eq!(watchdog.observe(event, 0, 0), None);
    }

    #[test]
    fn a_new_sequence_starts_a_new_watchdog_window() {
        let mut watchdog = InterruptCanaryWatchdog::default();
        for _ in 0..VTL2_INTERRUPT_CANARY_PENDING_POLLS {
            let _ = watchdog.observe(Some((GDMA_EQE_TEST_EVENT, 7)), 0, 0);
        }

        assert_eq!(watchdog.observe(Some((GDMA_EQE_TEST_EVENT, 8)), 0, 0), None);
        assert_eq!(watchdog.pending_polls, 1);
    }

    #[test]
    fn clearing_the_queue_resets_pending_detection() {
        let mut watchdog = InterruptCanaryWatchdog::default();
        for _ in 0..5 {
            assert_eq!(watchdog.observe(Some((GDMA_EQE_TEST_EVENT, 7)), 0, 0), None);
        }

        assert_eq!(watchdog.observe(None, 0, 0), None);
        assert_eq!(watchdog.pending_event, None);
        assert_eq!(watchdog.pending_polls, 0);
    }

    #[test]
    fn unexpected_event_is_classified() {
        let mut watchdog = InterruptCanaryWatchdog::default();
        let mut result = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_PENDING_POLLS {
            result = watchdog.observe(Some((0xff, 9)), 0, 0);
        }

        assert_eq!(
            result,
            Some((
                InterruptCanaryFailure::UnexpectedEqe,
                VTL2_INTERRUPT_CANARY_PENDING_POLLS * VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
                0,
            ))
        );
    }

    #[test]
    fn pending_eqe_after_interrupt_is_distinguished() {
        let mut watchdog = InterruptCanaryWatchdog::default();
        let event = Some((GDMA_EQE_TEST_EVENT, 7));

        assert_eq!(watchdog.observe(event, 1, 1), None);
        let mut result = None;
        for _ in 1..VTL2_INTERRUPT_CANARY_PENDING_POLLS {
            result = watchdog.observe(event, 1, 1);
        }

        assert_eq!(
            result,
            Some((
                InterruptCanaryFailure::EqePendingAfterInterrupt,
                VTL2_INTERRUPT_CANARY_PENDING_POLLS * VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
                1,
            ))
        );
    }

    #[test]
    fn interrupt_racing_with_empty_sample_is_not_lost() {
        let mut watchdog = InterruptCanaryWatchdog::default();
        assert_eq!(watchdog.observe(None, 0, 1), None);

        let event = Some((GDMA_EQE_TEST_EVENT, 7));
        let mut result = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_PENDING_POLLS {
            result = watchdog.observe(event, 1, 1);
        }

        assert_eq!(
            result,
            Some((
                InterruptCanaryFailure::EqePendingAfterInterrupt,
                VTL2_INTERRUPT_CANARY_PENDING_POLLS * VTL2_INTERRUPT_CANARY_POLL_INTERVAL_MS,
                1,
            ))
        );
    }

    #[test]
    fn polling_recovery_consumes_exact_eqe_and_rearms_after_quiet() {
        let (mut canary, eq_mem, _interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);

        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        assert_eq!(canary.expected_sequence, 2);
        assert!(canary.eq.peek().is_none());
        assert_eq!(canary.recovery_quiet_sequence, Some(1));
        assert_eq!(canary.poll_recovery_consecutive, 1);

        let mut control = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            control = canary.recovery_control();
        }
        let control = control.expect("quiet period should request rearm");
        canary.process_control(control);

        assert_eq!(canary.recovery_quiet_sequence, Some(1));
        assert!(canary.recovery_eq_armed);
        assert!(canary.poll_recovery_validation_pending);
    }

    #[test]
    fn polling_recovery_does_not_consume_after_late_signal() {
        let (mut canary, eq_mem, interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        let report = pending_report(&canary);
        interrupt_source.signal_uncached();

        canary.process_control(InterruptCanaryControl::RecoverPending(report));

        assert_eq!(canary.expected_sequence, 1);
        assert!(canary.eq.peek().is_some());
        assert_eq!(canary.recovery_quiet_sequence, None);
        assert_eq!(canary.poll_recovery_total, 0);
    }

    #[test]
    fn late_interrupt_restarts_polling_recovery_quiet_period() {
        let (mut canary, eq_mem, interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        for _ in 0..10 {
            assert!(canary.recovery_control().is_none());
        }
        assert_eq!(canary.recovery_quiet_polls, 10);

        interrupt_source.signal_uncached();
        assert!(canary.process_interrupt().is_none());

        assert_eq!(canary.recovery_quiet_polls, 0);
        assert_eq!(canary.recovery_quiet_signal_count, 1);
        assert!(!canary.recovery_arm_request_pending);
        assert_eq!(canary.poll_recovery_total, 0);
        assert_eq!(canary.poll_recovery_consecutive, 0);
        assert!(!canary.recovery_quiet_counted_as_poll);

        let mut control = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            control = canary.recovery_control();
        }
        canary.process_control(control.expect("restarted quiet period should request rearm"));
        assert!(canary.recovery_eq_armed);
        assert!(!canary.poll_recovery_validation_pending);

        post_test_eqe(&eq_mem, size_of::<Eqe>(), 2);
        interrupt_source.signal_uncached();
        assert!(canary.process_interrupt().is_none());
        assert_eq!(canary.expected_sequence, 3);
        assert_eq!(canary.recovery_quiet_sequence, None);
        assert!(!canary.recovery_eq_armed);
    }

    #[test]
    fn late_interrupt_after_rearm_disarms_and_restarts_quiet_period() {
        let (mut canary, eq_mem, interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        let mut control = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            control = canary.recovery_control();
        }
        canary.process_control(control.expect("quiet period should request rearm"));
        assert!(canary.recovery_eq_armed);

        interrupt_source.signal_uncached();
        assert!(canary.process_interrupt().is_none());

        assert!(!canary.recovery_eq_armed);
        assert!(!canary.poll_recovery_validation_pending);
        assert_eq!(canary.recovery_quiet_sequence, Some(1));
        assert_eq!(canary.recovery_quiet_polls, 0);
        assert_eq!(canary.recovery_quiet_signal_count, 1);
        assert_eq!(canary.poll_recovery_total, 0);
        assert_eq!(canary.poll_recovery_consecutive, 0);
        assert!(!canary.recovery_quiet_counted_as_poll);
    }

    #[test]
    fn hardware_interrupt_after_polling_recovery_resets_miss_streak() {
        let (mut canary, eq_mem, interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        let mut control = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            control = canary.recovery_control();
        }
        canary.process_control(control.expect("quiet period should request rearm"));

        post_test_eqe(&eq_mem, size_of::<Eqe>(), 2);
        interrupt_source.signal_uncached();
        assert!(canary.process_interrupt().is_none());

        assert_eq!(canary.expected_sequence, 3);
        assert_eq!(canary.poll_recovery_consecutive, 0);
        assert!(!canary.poll_recovery_validation_pending);
        assert_eq!(canary.recovery_quiet_sequence, None);
        assert!(!canary.recovery_eq_armed);
    }

    #[test]
    fn polling_recovery_stops_at_consecutive_limit() {
        let (mut canary, eq_mem, _interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        canary.poll_recovery_consecutive = VTL2_INTERRUPT_CANARY_MAX_CONSECUTIVE_POLL_RECOVERIES;

        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        assert!(!canary.monitoring_enabled());
        assert_eq!(canary.expected_sequence, 1);
        assert!(canary.eq.peek().is_some());
    }

    #[test]
    fn disabled_canary_does_not_consume_or_rearm() {
        let (mut canary, eq_mem, interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        canary.disable_monitoring();
        interrupt_source.signal_uncached();

        assert!(canary.process_interrupt().is_none());
        assert_eq!(canary.expected_sequence, 1);
        assert!(canary.eq.peek().is_some());
    }

    #[test]
    fn recovery_limit_does_not_disable_after_late_signal() {
        let (mut canary, eq_mem, interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        let report = pending_report(&canary);
        canary.poll_recovery_consecutive = VTL2_INTERRUPT_CANARY_MAX_CONSECUTIVE_POLL_RECOVERIES;
        interrupt_source.signal_uncached();

        canary.process_control(InterruptCanaryControl::RecoverPending(report));

        assert!(canary.monitoring_enabled());
        assert_eq!(canary.expected_sequence, 1);
        assert!(canary.eq.peek().is_some());
    }

    #[test]
    fn consecutive_lost_interrupts_each_complete_polling_recovery() {
        let (mut canary, eq_mem, _interrupt_source) = new_test_canary();
        post_test_eqe(&eq_mem, 0, 1);
        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        let mut control = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            control = canary.recovery_control();
        }
        canary.process_control(control.expect("first quiet period should request rearm"));
        assert!(canary.recovery_eq_armed);

        post_test_eqe(&eq_mem, size_of::<Eqe>(), 2);
        canary.process_control(InterruptCanaryControl::RecoverPending(pending_report(
            &canary,
        )));

        assert!(!canary.recovery_eq_armed);
        assert_eq!(canary.recovery_quiet_sequence, Some(2));
        assert_eq!(canary.poll_recovery_consecutive, 2);

        control = None;
        for _ in 0..VTL2_INTERRUPT_CANARY_RECOVERY_QUIET_POLLS {
            control = canary.recovery_control();
        }
        canary.process_control(control.expect("second quiet period should request rearm"));

        assert!(canary.recovery_eq_armed);
        assert!(canary.poll_recovery_validation_pending);
        assert_eq!(canary.expected_sequence, 3);
    }
}
