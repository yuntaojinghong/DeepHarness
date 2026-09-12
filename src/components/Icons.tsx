interface IconProps {
  size?: number;
}

const base = (size: number) => ({ width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const });

export const GearIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="12" cy="12" r="3.2" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1.11-1.56 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.56-1.11 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.08a1.7 1.7 0 0 0 1.03-1.56V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.08a1.7 1.7 0 0 0 1.56 1.03H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.56 1.03z" />
  </svg>
);

export const SunIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="12" cy="12" r="4.2" />
    <path d="M12 2v2.2M12 19.8V22M2 12h2.2M19.8 12H22M4.9 4.9l1.6 1.6M17.5 17.5l1.6 1.6M4.9 19.1l1.6-1.6M17.5 6.5l1.6-1.6" />
  </svg>
);

export const MoonIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M20.4 14.2A8.5 8.5 0 0 1 9.8 3.6a8.5 8.5 0 1 0 10.6 10.6z" />
  </svg>
);

export const PanelLeftIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <path d="M9 4v16" />
  </svg>
);

export const PanelRightIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <path d="M15 4v16" />
  </svg>
);

export const PlusIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const SearchIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="11" cy="11" r="7" />
    <path d="m21 21-4.3-4.3" />
  </svg>
);

export const TrashIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
  </svg>
);

export const CheckIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="m4 12.5 5 5L20 6.5" />
  </svg>
);

export const SendIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M22 2 11 13M22 2l-7 20-4-9-9-4 20-7z" />
  </svg>
);

export const ChevronDownIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="m6 9 6 6 6-6" />
  </svg>
);

export const CloseIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M18 6 6 18M6 6l12 12" />
  </svg>
);

export const FolderIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
  </svg>
);

export const CpuIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="5" y="5" width="14" height="14" rx="2" />
    <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
  </svg>
);

export const SparkIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M18.4 5.6l-2.8 2.8M8.4 15.6l-2.8 2.8" />
  </svg>
);

export const StopIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <rect x="6" y="6" width="12" height="12" rx="2" />
  </svg>
);

export const UserIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <circle cx="12" cy="8" r="3.6" />
    <path d="M5 20c.6-3.6 3.4-5.4 7-5.4s6.4 1.8 7 5.4" />
  </svg>
);

export const PowerIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M12 3v9" />
    <path d="M6.4 6.6a8 8 0 1 0 11.2 0" />
  </svg>
);

export const ExternalLinkIcon = ({ size = 16 }: IconProps) => (
  <svg {...base(size)}>
    <path d="M13 4h7v7" />
    <path d="M20 4 10.5 13.5" />
    <path d="M18 14.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h3.5" />
  </svg>
);

export const PythonIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" style={{ display: "block" }}>
    <rect width="24" height="24" rx="6" fill="#3776ab" />
    <path
      d="M12 5.6c1.7 0 2.3.2 2.3 1.3v1.5h-5v.5h6.2c1.4 0 2.1 1 2.1 2.6 0 1.5-.8 2.6-2.1 2.6h-1.1v-1.3c0-1.6-.5-2.4-2-2.4h-3.4c-1.2 0-2.1-1-2.1-2.2v-1c0-1.2 1-2.3 2.1-2.3z"
      fill="#ffffff"
    />
    <path
      d="M12 18.4c-1.7 0-2.3-.2-2.3-1.3v-1.5h5v-.5H8.5c-1.4 0-2.1-1-2.1-2.6 0-1.5.8-2.6 2.1-2.6h1.1v1.3c0 1.6.5 2.4 2 2.4h3.4c1.2 0 2.1 1 2.1 2.2v1c0 1.2-1 2.3-2.1 2.3z"
      fill="#ffd43b"
    />
  </svg>
);

export const NodeIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" style={{ display: "block" }}>
    <rect width="24" height="24" rx="6" fill="#3c873a" />
    <path
      d="M12 5.8 6.8 8.7v6.6L12 18.2l5.2-2.9V8.7z"
      fill="none"
      stroke="#ffffff"
      strokeWidth="1.5"
      strokeLinejoin="round"
    />
    <circle cx="12" cy="12" r="1.6" fill="#ffffff" />
  </svg>
);

export const GitIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" style={{ display: "block" }}>
    <rect width="24" height="24" rx="6" fill="#f05033" />
    <path d="M12 6.2 17.8 12 12 17.8 6.2 12z" fill="none" stroke="#ffffff" strokeWidth="1.5" strokeLinejoin="round" />
    <circle cx="12" cy="12" r="1.6" fill="#ffffff" />
    <path d="M6.2 12H4M12 17.8v2.2M17.8 12H20" stroke="#ffffff" strokeWidth="1.5" strokeLinecap="round" />
  </svg>
);
