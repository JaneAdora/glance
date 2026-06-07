//! Pure vitals logic for the `vitals` cockpit binary: alarm thresholds,
//! status classification, the status-line builder, the temperature combine, and
//! the responsive-mode selector. Kept out of the binary so it is unit-testable
//! and reusable across the suite.

/// Alarm thresholds (v1, hardcoded). Easy to lift into a config file later.
pub const CPU_ALARM: u16 = 90;
pub const RAM_ALARM: u16 = 90;
pub const GPU_ALARM: u16 = 90;
pub const TEMP_ALARM: f64 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Normal,
    Alarm,
    Unknown,
}

/// Classify an integer percentage reading. `None` -> `Unknown`. At or above the
/// threshold is `Alarm`.
pub fn classify_u16(value: Option<u16>, threshold: u16) -> Status {
    match value {
        None => Status::Unknown,
        Some(v) if v >= threshold => Status::Alarm,
        Some(_) => Status::Normal,
    }
}

/// Classify a temperature reading. `None` -> `Unknown`. At or above the
/// threshold is `Alarm`.
pub fn classify_temp(value: Option<f64>, threshold: f64) -> Status {
    match value {
        None => Status::Unknown,
        Some(v) if v >= threshold => Status::Alarm,
        Some(_) => Status::Normal,
    }
}

/// Hottest of the hottest thermal zone and the GPU temperature. Either may be
/// absent; `None` only when both are.
pub fn combine_temp(zone: Option<f64>, gpu_temp: Option<u16>) -> Option<f64> {
    let g = gpu_temp.map(|t| t as f64);
    match (zone, g) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Raw vitals readings. `None` means no sensor / unavailable.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vitals {
    pub cpu: Option<u16>,
    pub ram: Option<u16>,
    pub gpu: Option<u16>,
    pub temp: Option<f64>,
}

impl Vitals {
    pub fn cpu_status(&self) -> Status {
        classify_u16(self.cpu, CPU_ALARM)
    }
    pub fn ram_status(&self) -> Status {
        classify_u16(self.ram, RAM_ALARM)
    }
    pub fn gpu_status(&self) -> Status {
        classify_u16(self.gpu, GPU_ALARM)
    }
    pub fn temp_status(&self) -> Status {
        classify_temp(self.temp, TEMP_ALARM)
    }

    /// True if any metric is in alarm.
    pub fn any_alarm(&self) -> bool {
        self.cpu_status() == Status::Alarm
            || self.ram_status() == Status::Alarm
            || self.gpu_status() == Status::Alarm
            || self.temp_status() == Status::Alarm
    }
}

/// One-line status: "ALL NOMINAL" when nothing alarms, otherwise the offenders
/// in fixed order (GPU, CPU, RAM, TEMP) joined by "  ·  ". CPU/RAM/GPU are shown
/// as percentages, TEMP in celsius. Unknown metrics never appear.
pub fn status_line(v: &Vitals) -> String {
    let mut offenders: Vec<String> = Vec::new();
    if v.gpu_status() == Status::Alarm {
        offenders.push(format!("GPU {}%", v.gpu.unwrap()));
    }
    if v.cpu_status() == Status::Alarm {
        offenders.push(format!("CPU {}%", v.cpu.unwrap()));
    }
    if v.ram_status() == Status::Alarm {
        offenders.push(format!("RAM {}%", v.ram.unwrap()));
    }
    if v.temp_status() == Status::Alarm {
        offenders.push(format!("TEMP {:.0}°C", v.temp.unwrap()));
    }
    if offenders.is_empty() {
        "ALL NOMINAL".to_string()
    } else {
        offenders.join("  ·  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_u16_handles_none_normal_alarm() {
        assert_eq!(classify_u16(None, 90), Status::Unknown);
        assert_eq!(classify_u16(Some(89), 90), Status::Normal);
        assert_eq!(classify_u16(Some(90), 90), Status::Alarm);
        assert_eq!(classify_u16(Some(99), 90), Status::Alarm);
    }

    #[test]
    fn classify_temp_handles_none_normal_alarm() {
        assert_eq!(classify_temp(None, 80.0), Status::Unknown);
        assert_eq!(classify_temp(Some(79.9), 80.0), Status::Normal);
        assert_eq!(classify_temp(Some(80.0), 80.0), Status::Alarm);
    }

    #[test]
    fn combine_temp_takes_the_hotter() {
        assert_eq!(combine_temp(Some(50.0), Some(70)), Some(70.0));
        assert_eq!(combine_temp(Some(90.0), Some(70)), Some(90.0));
        assert_eq!(combine_temp(Some(55.0), None), Some(55.0));
        assert_eq!(combine_temp(None, Some(60)), Some(60.0));
        assert_eq!(combine_temp(None, None), None);
    }

    #[test]
    fn status_line_all_nominal_when_no_alarms() {
        let v = Vitals { cpu: Some(10), ram: Some(20), gpu: None, temp: Some(40.0) };
        assert_eq!(status_line(&v), "ALL NOMINAL");
    }

    #[test]
    fn status_line_lists_offenders_in_fixed_order() {
        let v = Vitals { cpu: Some(95), ram: Some(99), gpu: Some(94), temp: Some(85.0) };
        assert_eq!(status_line(&v), "GPU 94%  ·  CPU 95%  ·  RAM 99%  ·  TEMP 85°C");
    }

    #[test]
    fn status_line_unknown_never_appears() {
        let v = Vitals { cpu: Some(95), ram: None, gpu: None, temp: None };
        assert_eq!(status_line(&v), "CPU 95%");
    }

    #[test]
    fn any_alarm_reflects_metrics() {
        let calm = Vitals { cpu: Some(10), ram: Some(20), gpu: None, temp: None };
        assert!(!calm.any_alarm());
        let hot = Vitals { cpu: Some(10), ram: Some(20), gpu: None, temp: Some(95.0) };
        assert!(hot.any_alarm());
    }
}
