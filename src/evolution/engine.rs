use std::time::{SystemTime, Duration};

use chrono;

use crate::config::{EvolutionSection, ScheduleType, ApprovalMode, SafeMode};

pub struct EvolutionEngine {
    config: EvolutionConfig,
    iteration_count: usize,
    last_run_time: Option<SystemTime>,
    iterations_in_current_period: usize,
    last_failure_time: Option<SystemTime>,
}

#[derive(Clone)]
pub struct EvolutionConfig {
    pub enabled: bool,
    pub max_iterations: usize,
    pub target_score_threshold: f64,
    pub auto_commit: bool,
    pub require_approval: bool,
    pub focus_areas: Vec<String>,
    pub schedule_type: ScheduleType,
    pub schedule_interval_seconds: u64,
    pub schedule_time: String,
    pub max_iterations_per_period: usize,
    pub cooldown_seconds: u64,
    pub approval_mode: ApprovalMode,
    pub approval_timeout_seconds: u64,
    pub approval_webhook_url: Option<String>,
    pub require_approval_for: Vec<String>,
    pub safe_mode: SafeMode,
    pub allowed_directories: Vec<String>,
    pub restricted_files: Vec<String>,
    pub max_file_size_kb: usize,
    pub allowed_operation_types: Vec<String>,
    pub rollback_enabled: bool,
    pub backup_before_edit: bool,
}

impl From<EvolutionSection> for EvolutionConfig {
    fn from(section: EvolutionSection) -> Self {
        Self {
            enabled: section.enabled,
            max_iterations: section.max_iterations,
            target_score_threshold: section.target_score_threshold,
            auto_commit: section.auto_commit,
            require_approval: section.require_approval,
            focus_areas: section.focus_areas,
            schedule_type: section.schedule_type,
            schedule_interval_seconds: section.schedule_interval_seconds,
            schedule_time: section.schedule_time,
            max_iterations_per_period: section.max_iterations_per_period,
            cooldown_seconds: section.cooldown_seconds,
            approval_mode: section.approval_mode,
            approval_timeout_seconds: section.approval_timeout_seconds,
            approval_webhook_url: section.approval_webhook_url,
            require_approval_for: section.require_approval_for,
            safe_mode: section.safe_mode,
            allowed_directories: section.allowed_directories,
            restricted_files: section.restricted_files,
            max_file_size_kb: section.max_file_size_kb,
            allowed_operation_types: section.allowed_operation_types,
            rollback_enabled: section.rollback_enabled,
            backup_before_edit: section.backup_before_edit,
        }
    }
}

impl EvolutionEngine {
    pub fn new(config: EvolutionConfig) -> Self {
        Self {
            config,
            iteration_count: 0,
            last_run_time: None,
            iterations_in_current_period: 0,
            last_failure_time: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn can_continue(&self) -> bool {
        self.iteration_count < self.config.max_iterations
    }

    pub fn increment_iteration(&mut self) {
        self.iteration_count += 1;
    }

    pub fn current_iteration(&self) -> usize {
        self.iteration_count
    }

    pub fn config(&self) -> &EvolutionConfig {
        &self.config
    }

    pub fn should_run_now(&mut self) -> bool {
        // 检查冷却时间
        if let Some(last_failure) = self.last_failure_time {
            let cooldown_duration = Duration::from_secs(self.config.cooldown_seconds);
            if let Ok(elapsed) = last_failure.elapsed() {
                if elapsed < cooldown_duration {
                    return false;
                }
            }
        }

        // 检查调度
        match self.config.schedule_type {
            ScheduleType::Manual => false, // 仅手动触发
            ScheduleType::Interval => self.should_run_interval(),
            ScheduleType::Daily => self.should_run_daily(),
            ScheduleType::Weekly => self.should_run_weekly(),
        }
    }

    fn should_run_interval(&self) -> bool {
        if let Some(last_run) = self.last_run_time {
            let interval_duration = Duration::from_secs(self.config.schedule_interval_seconds);
            if let Ok(elapsed) = last_run.elapsed() {
                return elapsed >= interval_duration && 
                       self.iterations_in_current_period < self.config.max_iterations_per_period;
            }
        }
        // 从未运行过，或者无法获取时间
        true
    }

    fn should_run_daily(&self) -> bool {
        self.should_run_at_scheduled_time("daily")
    }

    fn should_run_weekly(&self) -> bool {
        self.should_run_at_scheduled_time("weekly")
    }

    fn should_run_at_scheduled_time(&self, period: &str) -> bool {
        // 解析计划时间 HH:MM
        let (sched_hour, sched_minute) = match self.parse_schedule_time() {
            Some((h, m)) => (h, m),
            None => {
                eprintln!("⚠️ 无法解析计划时间 '{}'，使用默认时间 02:00", self.config.schedule_time);
                (2, 0) // 默认 02:00
            }
        };

        let now = chrono::Local::now();
        let today = now.date_naive();
        
        // 创建今天的计划时间
        let schedule_today = match chrono::NaiveTime::from_hms_opt(sched_hour, sched_minute, 0) {
            Some(t) => chrono::NaiveDateTime::new(today, t),
            None => {
                eprintln!("⚠️ 无效的计划时间 {}:{}, 跳过本次运行", sched_hour, sched_minute);
                return false;
            }
        };

        // 检查是否已经过了今天的计划时间
        let now_naive = now.naive_local();
        if now_naive < schedule_today {
            // 还没到计划时间
            return false;
        }

        // 检查上次运行时间
        if let Some(last_run) = self.last_run_time {
            // 将 SystemTime 转换为 chrono::DateTime<Local>
            let last_run_datetime: chrono::DateTime<chrono::Local> = match last_run.duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => {
                    // Convert seconds to DateTime<Utc> then to Local
                    chrono::DateTime::from_timestamp(duration.as_secs() as i64, 0)
                        .map(|utc| utc.with_timezone(&chrono::Local))
                        .unwrap_or_else(chrono::Local::now)
                }
                Err(_) => {
                    // 如果时间在 UNIX_EPOCH 之前，使用当前时间
                    chrono::Local::now()
                }
            };
            
            let last_run_naive = last_run_datetime.naive_local();
            
            // 检查是否已经在当前周期内运行过
            match period {
                "daily" => {
                    // 如果上次运行在今天计划时间之后，说明已经运行过了
                    if last_run_naive >= schedule_today {
                        return self.iterations_in_current_period < self.config.max_iterations_per_period;
                    }
                }
                "weekly" => {
                    // 对于每周调度，检查是否在过去7天内运行过
                    let one_week = chrono::Duration::days(7);
                    let one_week_ago = now_naive - one_week;
                    if last_run_naive >= one_week_ago {
                        return self.iterations_in_current_period < self.config.max_iterations_per_period;
                    }
                }
                _ => {}
            }
        }

        // 新周期开始，但还需要检查是否超过最大迭代次数
        if self.iterations_in_current_period >= self.config.max_iterations_per_period {
            return false;
        }

        true
    }
    
    fn parse_schedule_time(&self) -> Option<(u32, u32)> {
        let parts: Vec<&str> = self.config.schedule_time.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        
        let hour = parts[0].parse::<u32>().ok()?;
        let minute = parts[1].parse::<u32>().ok()?;
        
        if hour > 23 || minute > 59 {
            return None;
        }
        
        Some((hour, minute))
    }

    pub fn record_successful_run(&mut self) {
        self.last_run_time = Some(SystemTime::now());
        self.iterations_in_current_period += 1;
        self.last_failure_time = None;
    }

    pub fn record_failed_run(&mut self) {
        self.last_failure_time = Some(SystemTime::now());
    }

    pub fn reset_period_if_needed(&mut self) {
        // 简化实现：每天重置
        let one_day = Duration::from_secs(86400);
        if let Some(last_run) = self.last_run_time {
            if let Ok(elapsed) = last_run.elapsed() {
                if elapsed >= one_day {
                    self.iterations_in_current_period = 0;
                }
            }
        }
    }

    pub fn is_in_cooldown(&self) -> bool {
        if let Some(last_failure) = self.last_failure_time {
            let cooldown_duration = Duration::from_secs(self.config.cooldown_seconds);
            if let Ok(elapsed) = last_failure.elapsed() {
                return elapsed < cooldown_duration;
            }
        }
        false
    }
}
