import { ArrowLeft, Bookmark, Clock, Heart, Share2 } from "lucide-react";
import CommentSection from "./CommentSection";

export default function PostDetail({
  post,
  onBack,
  onToggleLike,
  onToggleBookmark,
  onShowNotification,
  currentUser,
  onAddComment,
  onOpenAuth,
}) {
  return (
    <div className="max-w-4xl mx-auto space-y-8 animate-fadeIn">
      <button
        onClick={onBack}
        className="inline-flex items-center gap-2 text-xs font-semibold text-slate-500 hover:text-slate-900 transition-colors bg-white px-4 py-2 rounded-full border border-slate-200 shadow-sm"
      >
        <ArrowLeft className="w-4 h-4" />
        Back to feed
      </button>

      <article className="bg-white rounded-3xl border border-slate-200/80 overflow-hidden shadow-sm">
        <div className="h-72 sm:h-96 w-full relative overflow-hidden bg-slate-100">
          <img
            src={post.coverImage}
            alt={post.title}
            className="w-full h-full object-cover"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-black/60 via-transparent to-transparent" />
          <span className="absolute top-6 left-6 bg-white/90 backdrop-blur-md text-slate-900 font-semibold text-xs px-3 py-1.5 rounded-full shadow-sm">
            {post.category}
          </span>
        </div>

        <div className="p-6 sm:p-10 space-y-6">
          <h1 className="text-2xl sm:text-4xl font-extrabold text-slate-900 tracking-tight leading-tight">
            {post.title}
          </h1>

          <div className="flex items-center justify-between pb-6 border-b border-slate-100 flex-wrap gap-4">
            <div className="flex items-center gap-3">
              <img
                src={post.author.avatar}
                alt={post.author.name}
                className="w-12 h-12 rounded-full object-cover ring-2 ring-indigo-50"
              />
              <div>
                <h4 className="font-semibold text-slate-900 text-xs sm:text-sm">
                  {post.author.name}
                </h4>
                <p className="text-[11px] text-slate-500">{post.author.role}</p>
              </div>
            </div>

            <div className="flex items-center gap-4 text-xs text-slate-500">
              <span className="flex items-center gap-1.5">
                <Clock className="w-4 h-4 text-slate-400" />
                {post.date} · {post.readTime}
              </span>
            </div>
          </div>

          <div className="prose prose-slate max-w-none text-slate-700 leading-relaxed text-sm sm:text-base space-y-4 whitespace-pre-line">
            {post.content}
          </div>

          <div className="flex items-center justify-between pt-8 border-t border-slate-100">
            <div className="flex items-center gap-3">
              <button
                onClick={(e) => onToggleLike(post.id, e)}
                className={`flex items-center gap-2 px-4 py-2 rounded-full text-xs font-bold transition-all ${
                  post.isLiked
                    ? "bg-rose-50 text-rose-600 border border-rose-200"
                    : "bg-slate-100 hover:bg-slate-200 text-slate-700"
                }`}
              >
                <Heart
                  className={`w-4 h-4 ${post.isLiked ? "fill-rose-600 text-rose-600" : ""}`}
                />
                <span>{post.likes} Likes</span>
              </button>

              <button
                onClick={(e) => onToggleBookmark(post.id, e)}
                className={`p-2.5 rounded-full transition ${
                  post.isBookmarked
                    ? "bg-amber-50 text-amber-600 border border-amber-200"
                    : "bg-slate-100 hover:bg-slate-200 text-slate-600"
                }`}
                title="Bookmark Article"
              >
                <Bookmark
                  className={`w-4 h-4 ${post.isBookmarked ? "fill-amber-500 text-amber-500" : ""}`}
                />
              </button>
            </div>

            <button
              onClick={() => {
                navigator.clipboard?.writeText?.(window.location.href);
                onShowNotification("Article link copied to clipboard!");
              }}
              className="flex items-center gap-2 text-xs text-slate-500 hover:text-indigo-600 px-3 py-2 rounded-lg transition font-semibold"
            >
              <Share2 className="w-4 h-4" />
              <span className="hidden sm:inline">Share</span>
            </button>
          </div>
        </div>
      </article>

      <CommentSection
        comments={post.comments}
        currentUser={currentUser}
        onAddComment={onAddComment}
        onOpenAuth={onOpenAuth}
      />
    </div>
  );
}
