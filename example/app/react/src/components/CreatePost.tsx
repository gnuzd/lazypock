import { ArrowLeft } from "lucide-react";
import { useState } from "react";

export default function CreatePost({
  onCancel,
  onSubmit,
  categories,
  currentUser,
}) {
  const [title, setTitle] = useState("");
  const [category, setCategory] = useState("Engineering");
  const [excerpt, setExcerpt] = useState("");
  const [content, setContent] = useState("");

  const handleFormSubmit = (e) => {
    e.preventDefault();
    if (!title || !content) return;
    onSubmit({ title, category, excerpt, content });
  };

  return (
    <div className="max-w-3xl mx-auto bg-white rounded-3xl border border-slate-200 p-6 sm:p-10 shadow-sm animate-fadeIn">
      <div className="flex items-center justify-between mb-8 pb-4 border-b border-slate-100">
        <button
          onClick={onCancel}
          className="flex items-center gap-2 text-xs font-semibold text-slate-500 hover:text-slate-800 transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
          Back to Feed
        </button>
        <h2 className="text-xl font-bold text-slate-900">Create Article</h2>
      </div>

      <form onSubmit={handleFormSubmit} className="space-y-6">
        <div className="flex items-center gap-3 p-4 bg-slate-50 rounded-2xl border border-slate-100">
          <img
            src={currentUser.avatar}
            alt={currentUser.name}
            className="w-10 h-10 rounded-full"
          />
          <div>
            <span className="text-xs font-bold text-slate-900 block">
              {currentUser.name}
            </span>
            <span className="text-[11px] text-slate-500">
              {currentUser.role}
            </span>
          </div>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div className="sm:col-span-2">
            <label className="block text-xs font-semibold text-slate-700 mb-1">
              Article Title *
            </label>
            <input
              type="text"
              required
              placeholder="Enter an engaging title..."
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              className="w-full px-4 py-2.5 rounded-xl border border-slate-200 focus:border-indigo-500 text-xs outline-none transition"
            />
          </div>
          <div>
            <label className="block text-xs font-semibold text-slate-700 mb-1">
              Category
            </label>
            <select
              value={category}
              onChange={(e) => setCategory(e.target.value)}
              className="w-full px-4 py-2.5 rounded-xl border border-slate-200 focus:border-indigo-500 text-xs outline-none bg-white transition"
            >
              {categories
                .filter((c) => c !== "All")
                .map((cat) => (
                  <option key={cat} value={cat}>
                    {cat}
                  </option>
                ))}
            </select>
          </div>
        </div>

        <div>
          <label className="block text-xs font-semibold text-slate-700 mb-1">
            Short Excerpt
          </label>
          <input
            type="text"
            placeholder="Brief summary for feed preview..."
            value={excerpt}
            onChange={(e) => setExcerpt(e.target.value)}
            className="w-full px-4 py-2.5 rounded-xl border border-slate-200 focus:border-indigo-500 text-xs outline-none transition"
          />
        </div>

        <div>
          <label className="block text-xs font-semibold text-slate-700 mb-1">
            Content *
          </label>
          <textarea
            required
            rows={10}
            placeholder="Write your article content here..."
            value={content}
            onChange={(e) => setContent(e.target.value)}
            className="w-full px-4 py-3 rounded-xl border border-slate-200 focus:border-indigo-500 text-xs outline-none transition font-mono"
          />
        </div>

        <div className="flex items-center justify-end gap-3 pt-4 border-t border-slate-100">
          <button
            type="button"
            onClick={onCancel}
            className="px-5 py-2.5 rounded-full border border-slate-200 text-slate-600 hover:bg-slate-50 text-xs font-semibold transition"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="px-6 py-2.5 bg-indigo-600 hover:bg-indigo-700 text-white rounded-full text-xs font-bold shadow-lg shadow-indigo-100 active:scale-95 transition"
          >
            Publish Article
          </button>
        </div>
      </form>
    </div>
  );
}
