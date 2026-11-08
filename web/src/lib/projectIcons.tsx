// A project's icon and accent. The names match ICONS in
// crates/master/src/projects.rs (projects.test.ts checks); the accent is one
// of the app's accent tokens, so it follows the theme.
import {
  BookOpen,
  Briefcase,
  Camera,
  ChartLine,
  Code,
  Film,
  FlaskConical,
  Folder,
  Gamepad2,
  Globe,
  GraduationCap,
  Heart,
  House,
  Lightbulb,
  type LucideIcon,
  Music,
  Palette,
  PenTool,
  Plane,
  Rocket,
  Terminal,
} from "lucide-react";

export const PROJECT_ICONS: Record<string, LucideIcon> = {
  folder: Folder,
  briefcase: Briefcase,
  code: Code,
  terminal: Terminal,
  rocket: Rocket,
  "book-open": BookOpen,
  "graduation-cap": GraduationCap,
  "flask-conical": FlaskConical,
  lightbulb: Lightbulb,
  "pen-tool": PenTool,
  palette: Palette,
  camera: Camera,
  music: Music,
  film: Film,
  "gamepad-2": Gamepad2,
  heart: Heart,
  house: House,
  plane: Plane,
  "chart-line": ChartLine,
  globe: Globe,
};

/** The CSS colour of an accent name (unknown names fall back to the app accent). */
export function accentColor(accent: string): string {
  return /^[a-z]+$/.test(accent) ? `var(--accent-${accent}, var(--accent))` : "var(--accent)";
}

export function ProjectIcon({ icon, accent, size = 16 }: { icon: string; accent: string; size?: number }) {
  const Icon = PROJECT_ICONS[icon] ?? Folder;
  return <Icon size={size} className="project-icon" style={{ color: accentColor(accent) }} aria-hidden />;
}
