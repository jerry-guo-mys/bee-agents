//! 认知层：Planner、Critic、ReAct 主循环、三层记忆协调（ContextManager）

pub mod critic;
pub mod events;
pub mod loop_;
pub mod memory;
pub mod planner;

pub use critic::Critic;
pub use events::ReactEvent;
pub use loop_::react_loop;
pub use memory::ContextManager;
pub use planner::{parse_llm_output, Planner};
