import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store";
import { useAgentStatus } from "../lib/hooks";
import {
  agentStart,
  deepharnessPlan,
  deepharnessRunTask,
  describeError,
  formatArgs,
  truncateResult,
  type Plan,
  type PlanStep,
  type StepRecord,
  type TaskOutcome,
  type WorkerReadiness,
} from "../lib/deepharness";
import { CheckIcon, CloseIcon, SparkIcon, StopIcon } from "./Icons";
import LogoMark from "./Logo";

/**
 * DeepHarness 原生 Agent 的任务控制台。
 *
 * 与另外两个 Agent 不同，DeepHarness 不使用「对话」形态，而是显式的
 * 三段式任务流：目标 → 结构化计划 → 逐步执行（工具受权限白名单约束）
 * → 反思 → 记忆沉淀。本组件只负责呈现与触发，全部执行都在 Rust 侧的
 * Worker 中完成。
 */
export default function DeepHarnessView() {
  const activeAgent = useAppStore((s) => s.activeAgent);
  const agentStatus = useAgentStatus("deepharness");
  const readiness = useAppStore((s) => s.workerReadiness);
  const refreshAgents = useAppStore((s) => s.refreshAgents);
  const refreshWorkerReadiness = useAppStore((s) => s.refreshWorkerReadiness);
  const setContextOpen = useAppStore((s) => s.setContextOpen);

  const [goal, setGoal] = useState("");
  const [busy, setBusy] = useState<null | "plan" | "task">(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [outcome, setOutcome] = useState<TaskOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const scrollRef = useRef<HTMLDivElement>(null);

  const running = agentStatus?.state === "running";
  const configured = readiness?.configured === true;

  // 切换到本 Agent 时立刻取一次状态，不必等 App 的 5 秒轮询周期。
  useEffect(() => {
    if (activeAgent !== "deepharness") return;
    void refreshWorkerReadiness();
  }, [activeAgent, refreshWorkerReadiness]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [plan, outcome]);

  /**
   * 确保 Worker 在跑，并返回**这次重新读到的**就绪状态。
   *
   * 必须返回新值：刚启动时组件里的 `readiness` 还是上一轮 render 的旧闭包，
   * 直接用它判断会误报「未配置模型」。
   */
  const ensureWorker = async (): Promise<WorkerReadiness | null> => {
    if (running) return refreshWorkerReadiness();
    setStarting(true);
    setNotice("正在启动 DeepHarness Worker…");
    try {
      await agentStart("deepharness");
      await refreshAgents();
      const wr = await refreshWorkerReadiness();
      setNotice("Worker 已启动");
      return wr;
    } catch (e) {
      const msg = describeError(e);
      console.error("[deepharness] 启动 Worker 失败", e);
      setNotice(null);
      setError(`Worker 启动失败：${msg}`);
      return null;
    } finally {
      setStarting(false);
    }
  };

  /** 统一的前置检查：Worker 可用且已配置模型。 */
  const prepare = async (): Promise<boolean> => {
    setError(null);
    setNotice(null);
    const wr = await ensureWorker();
    if (!wr) return false;
    if (!wr.configured) {
      setError("尚未配置模型参数，请先在右侧「模型配置」中填写 Base URL / API Key / 模型名。");
      setContextOpen(true);
      return false;
    }
    return true;
  };

  const handlePlan = async () => {
    const text = goal.trim();
    if (!text) {
      setError("请先填写任务目标。");
      return;
    }
    setOutcome(null);
    if (!(await prepare())) return;
    setBusy("plan");
    try {
      const p = await deepharnessPlan(text);
      setPlan(p);
      setNotice(`已生成 ${p.steps.length} 步计划，确认无误后可执行。`);
    } catch (e) {
      console.error("[deepharness] 生成计划失败", e);
      setError(`生成计划失败：${describeError(e)}`);
    } finally {
      setBusy(null);
    }
  };

  const handleRun = async () => {
    const text = goal.trim();
    if (!text) {
      setError("请先填写任务目标。");
      return;
    }
    if (!(await prepare())) return;
    setBusy("task");
    setOutcome(null);
    setNotice("任务执行中，可能需要数十秒，请稍候…");
    try {
      const result = await deepharnessRunTask(text);
      setOutcome(result);
      setNotice(result.success ? "任务完成" : "任务已结束，但存在失败步骤，请查看下方明细。");
    } catch (e) {
      console.error("[deepharness] 执行任务失败", e);
      setError(`执行任务失败：${describeError(e)}`);
      setNotice(null);
    } finally {
      setBusy(null);
    }
  };

  const reset = () => {
    setGoal("");
    setPlan(null);
    setOutcome(null);
    setError(null);
    setNotice(null);
  };

  const steps: StepRecord[] = outcome?.steps ?? [];
  const hasResult = Boolean(plan || outcome);

  return (
    <div className="main">
      <div className="dh-head">
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span className="dh-agent-title">DeepHarness 任务控制台</span>
          <span className="badge" title="由 Rust 侧 Worker 上报">
            <span className={`badge-dot ${running ? "ok" : readiness === null ? "busy" : ""}`} />
            {running ? (configured ? "Worker 已就绪" : "Worker 运行中 · 未配置模型") : "Worker 未运行"}
          </span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {hasResult && (
            <button className="btn btn-sm btn-ghost" onClick={reset} disabled={busy !== null}>
              清空结果
            </button>
          )}
          <button className="btn btn-sm" onClick={() => setContextOpen(true)}>
            模型与记忆
          </button>
        </div>
      </div>

      <div className="dh-scroll" ref={scrollRef}>
        <div className="dh-goal-card">
          <div className="section-title" style={{ padding: "0 0 6px" }}>
            任务目标
          </div>
          <textarea
            className="form-input dh-goal-input"
            placeholder="描述你希望完成的任务，例如：统计当前工作区内所有 Markdown 文件的字数并生成汇总报告"
            value={goal}
            rows={3}
            disabled={busy !== null}
            onChange={(e) => setGoal(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                e.preventDefault();
                void handleRun();
              }
            }}
          />
          <div className="dh-goal-actions">
            <span className="dh-goal-hint">
              计划只读不执行；执行会按白名单调用工具并沉淀记忆。Ctrl / ⌘ + Enter 直接执行。
            </span>
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn btn-sm" onClick={handlePlan} disabled={busy !== null || starting}>
                {busy === "plan" ? <span className="spinner" /> : <SparkIcon size={13} />}
                生成计划
              </button>
              <button className="btn btn-sm btn-primary" onClick={handleRun} disabled={busy !== null || starting}>
                {busy === "task" ? <span className="spinner" /> : <StopIcon size={13} />}
                执行任务
              </button>
            </div>
          </div>
        </div>

        {error && (
          <div className="dh-alert dh-alert-error">
            <span style={{ flex: 1 }}>{error}</span>
            <button className="btn-icon btn-ghost" style={{ width: 22, height: 22 }} onClick={() => setError(null)}>
              <CloseIcon size={12} />
            </button>
          </div>
        )}
        {notice && <div className="dh-alert dh-alert-info">{notice}</div>}

        {!hasResult && busy === null && (
          <div className="dh-empty">
            <div className="logo-float">
              <LogoMark size={52} radius={16} />
            </div>
            <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text)" }}>从一句目标开始</div>
            <div style={{ fontSize: 13, lineHeight: 1.8, maxWidth: 460 }}>
              先「生成计划」检视模型打算怎么做，确认后再「执行任务」。
              <br />
              每一步的工具调用都会经过该 Agent 独立的权限白名单。
            </div>
          </div>
        )}

        {outcome && (
          <div className="dh-summary-card">
            <div className="dh-summary-head">
              <span className={`dh-pill ${outcome.success ? "ok" : "err"}`}>
                {outcome.success ? <CheckIcon size={12} /> : <CloseIcon size={12} />}
                {outcome.success ? "全部步骤成功" : "存在失败步骤"}
              </span>
              <span className="dh-summary-goal">{outcome.goal}</span>
            </div>
            <p className="dh-summary-text">{outcome.summary}</p>
          </div>
        )}

        {outcome ? (
          <div>
            <div className="section-title" style={{ padding: "4px 0 8px" }}>
              执行明细（{steps.length} 步）
            </div>
            <div className="dh-timeline">
              {steps.map((s, i) => (
                <StepCard key={`${i}-${s.title}`} index={i + 1} step={s} />
              ))}
            </div>
          </div>
        ) : plan ? (
          <div>
            <div className="section-title" style={{ padding: "4px 0 8px" }}>
              计划预览（{plan.steps.length} 步 · 尚未执行）
            </div>
            <div className="dh-timeline">
              {plan.steps.map((s, i) => (
                <PlanCard key={`${i}-${s.title}`} index={i + 1} step={s} />
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** 计划步骤卡片：仅描述打算做什么，不含执行结果。 */
function PlanCard({ index, step }: { index: number; step: PlanStep }) {
  return (
    <div className="dh-step">
      <div className="dh-step-marker">{index}</div>
      <div className="dh-step-body">
        <div className="dh-step-head">
          <span className="dh-step-title">{step.title}</span>
          <span className={`dh-pill ${step.tool ? "tool" : "answer"}`}>
            {step.tool ? `工具 · ${step.tool}` : "回答"}
          </span>
        </div>
        {step.expected && <div className="dh-step-line">预期：{step.expected}</div>}
        {step.tool && (
          <details className="dh-details">
            <summary>参数</summary>
            <pre>{formatArgs(step.args)}</pre>
          </details>
        )}
      </div>
    </div>
  );
}

/** 执行步骤卡片：工具、结果、反思、重试与修订标注。 */
function StepCard({ index, step }: { index: number; step: StepRecord }) {
  const review = step.review;
  return (
    <div className={`dh-step ${step.ok ? "" : "failed"}`}>
      <div className="dh-step-marker">{step.ok ? <CheckIcon size={12} /> : <CloseIcon size={12} />}</div>
      <div className="dh-step-body">
        <div className="dh-step-head">
          <span className="dh-step-title">
            {index}. {step.title}
          </span>
          <span className={`dh-pill ${step.tool ? "tool" : "answer"}`}>
            {step.tool ?? "回答"}
          </span>
          {step.retried && <span className="dh-pill warn">已重试</span>}
          {step.revised && <span className="dh-pill info">步骤已修订</span>}
        </div>

        {review?.summary && <div className="dh-step-line">反思：{review.summary}</div>}
        {!review?.success && review?.advice && <div className="dh-step-line warn">建议：{review.advice}</div>}

        <details className="dh-details">
          <summary>{step.ok ? "查看结果" : "查看错误"}</summary>
          <pre>{truncateResult(step.result)}</pre>
        </details>
      </div>
    </div>
  );
}
