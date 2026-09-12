import { useEffect, useRef, useState } from "react";
import { useAppStore } from "../store";
import { useActiveConversation } from "../lib/hooks";
import { PERSONAS } from "../lib/storage";
import { ChevronDownIcon } from "./Icons";

export default function PersonaMenu() {
  const conv = useActiveConversation();
  const setPersona = useAppStore((s) => s.setPersona);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const current = PERSONAS.find((p) => p.id === conv?.systemPromptId) ?? PERSONAS[0];
  if (!current) return null;

  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <button
        className="btn btn-sm"
        style={{ gap: 6, paddingLeft: 8 }}
        onClick={() => setOpen(!open)}
        title="切换人设（System Prompt）"
      >
        <span style={{ width: 8, height: 8, borderRadius: 3, background: current.color, display: "inline-block" }} />
        {current.name}
        <ChevronDownIcon size={12} />
      </button>
      {open && (
        <div className="menu" style={{ top: "calc(100% + 6px)", right: 0, left: "auto", minWidth: 180 }}>
          <div className="section-title" style={{ padding: "4px 10px" }}>人设卡</div>
          {PERSONAS.map((p) => (
            <button
              key={p.id}
              className={`menu-item ${p.id === current.id ? "active" : ""}`}
              onClick={() => {
                setPersona(p.id);
                setOpen(false);
              }}
            >
              <span style={{ width: 9, height: 9, borderRadius: 3, background: p.color, flexShrink: 0, display: "inline-block" }} />
              <span style={{ flex: 1, textAlign: "left" }}>{p.name}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
