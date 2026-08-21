import { CheckCircle2 } from "lucide-react";

export default function Toast({ message }) {
  if (!message) return null;

  return (
    <div className="fixed bottom-6 right-6 z-50 flex items-center gap-2 bg-slate-900 text-white px-4 py-3 rounded-xl shadow-2xl border border-slate-700 animate-bounce">
      <CheckCircle2 className="w-5 h-5 text-emerald-400" />
      <span className="text-sm font-medium">{message}</span>
    </div>
  );
}
