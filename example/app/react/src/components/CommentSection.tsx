import { MessageSquare, Send } from "lucide-react";
import { useState } from "react";

export default function CommentSection({
  comments,
  currentUser,
  onAddComment,
  onOpenAuth,
}) {
  const [commentText, setCommentText] = useState("");

  const handleSubmit = (e) => {
    e.preventDefault();
    if (!commentText.trim()) return;

    if (!currentUser) {
      onOpenAuth("login");
      return;
    }

    onAddComment(commentText);
    setCommentText("");
  };

  return (
    <section className="bg-white rounded-3xl border border-slate-200/80 p-6 sm:p-10 shadow-sm space-y-8">
      <div className="flex items-center justify-between">
        <h3 className="text-xl font-bold text-slate-900 flex items-center gap-2">
          <MessageSquare className="w-5 h-5 text-indigo-600" />
          Discussion ({comments.length})
        </h3>
      </div>

      {/* Form */}
      <form
        onSubmit={handleSubmit}
        className="bg-slate-50 p-4 sm:p-6 rounded-2xl border border-slate-200/60 space-y-4"
      >
        {currentUser ? (
          <div className="flex items-center gap-3 pb-2 border-b border-slate-200/60">
            <img
              src={currentUser.avatar}
              alt={currentUser.name}
              className="w-8 h-8 rounded-full"
            />
            <div>
              <span className="text-xs font-bold text-slate-900 block">
                {currentUser.name}
              </span>
              <span className="text-[10px] text-slate-400">
                {currentUser.role}
              </span>
            </div>
          </div>
        ) : (
          <div className="p-3 bg-amber-50 border border-amber-200/70 rounded-xl text-amber-800 text-xs flex items-center justify-between gap-2">
            <span>
              You are commenting as a guest. Please sign in for full author
              features.
            </span>
            <button
              type="button"
              onClick={() => onOpenAuth("login")}
              className="px-3 py-1 bg-amber-600 text-white rounded-lg text-xs font-semibold shrink-0"
            >
              Sign In
            </button>
          </div>
        )}

        <div>
          <textarea
            required
            rows={3}
            placeholder="What are your thoughts on this article?"
            value={commentText}
            onChange={(e) => setCommentText(e.target.value)}
            className="w-full px-4 py-3 rounded-xl bg-white border border-slate-200 focus:border-indigo-500 text-xs outline-none transition"
          />
        </div>

        <div className="flex justify-end">
          <button
            type="submit"
            className="flex items-center gap-2 px-5 py-2.5 bg-indigo-600 hover:bg-indigo-700 text-white rounded-xl text-xs font-semibold shadow-md shadow-indigo-100 active:scale-95 transition"
          >
            <Send className="w-3.5 h-3.5" />
            <span>Post Comment</span>
          </button>
        </div>
      </form>

      {/* Comments List */}
      <div className="space-y-4 pt-2">
        {comments.length === 0 ? (
          <div className="text-center py-8 text-slate-400 text-xs">
            No comments yet. Be the first to start the conversation!
          </div>
        ) : (
          comments.map((comment) => (
            <div
              key={comment.id}
              className="p-4 sm:p-5 rounded-2xl bg-slate-50/50 border border-slate-100 space-y-2"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <img
                    src={comment.avatar}
                    alt={comment.author}
                    className="w-8 h-8 rounded-full bg-indigo-100 object-cover"
                  />
                  <div>
                    <span className="font-semibold text-slate-900 text-xs block">
                      {comment.author}
                    </span>
                    <span className="text-[10px] text-slate-400">
                      {comment.date}
                    </span>
                  </div>
                </div>
              </div>
              <p className="text-xs sm:text-sm text-slate-700 pl-11 leading-relaxed">
                {comment.text}
              </p>
            </div>
          ))
        )}
      </div>
    </section>
  );
}
