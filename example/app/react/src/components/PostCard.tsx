import { Bookmark, Heart, MessageSquare } from "lucide-react";

export default function PostCard({
  post,
  onSelect,
  onToggleLike,
  onToggleBookmark,
}) {
  return (
    <article
      onClick={() => onSelect(post.id)}
      className="group bg-white rounded-2xl border border-slate-200/80 overflow-hidden shadow-sm hover:shadow-xl hover:-translate-y-1 transition-all duration-300 cursor-pointer flex flex-col justify-between"
    >
      <div>
        <div className="h-48 w-full relative overflow-hidden bg-slate-100">
          <img
            src={post.coverImage}
            alt={post.title}
            className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
          />
          <span className="absolute top-3 left-3 bg-white/90 backdrop-blur-md text-slate-900 font-semibold text-[10px] uppercase tracking-wider px-2.5 py-1 rounded-full shadow-sm">
            {post.category}
          </span>
        </div>

        <div className="p-5 space-y-3">
          <h3 className="font-bold text-slate-900 text-lg leading-snug group-hover:text-indigo-600 transition-colors line-clamp-2">
            {post.title}
          </h3>
          <p className="text-xs text-slate-500 line-clamp-3 leading-relaxed">
            {post.excerpt}
          </p>
        </div>
      </div>

      <div className="p-5 pt-0 space-y-4">
        <div className="flex items-center justify-between border-t border-slate-100 pt-4">
          <div className="flex items-center gap-2">
            <img
              src={post.author.avatar}
              alt={post.author.name}
              className="w-7 h-7 rounded-full object-cover"
            />
            <span className="text-xs font-medium text-slate-700">
              {post.author.name}
            </span>
          </div>
          <span className="text-[11px] text-slate-400">{post.readTime}</span>
        </div>

        <div className="flex items-center justify-between text-xs text-slate-500">
          <div className="flex items-center gap-3">
            <button
              onClick={(e) => onToggleLike(post.id, e)}
              className="flex items-center gap-1 hover:text-rose-600 transition-colors"
            >
              <Heart
                className={`w-3.5 h-3.5 ${post.isLiked ? "fill-rose-500 text-rose-500" : ""}`}
              />
              <span>{post.likes}</span>
            </button>

            <span className="flex items-center gap-1">
              <MessageSquare className="w-3.5 h-3.5 text-slate-400" />
              <span>{post.comments.length}</span>
            </span>
          </div>

          <button
            onClick={(e) => onToggleBookmark(post.id, e)}
            className="hover:text-amber-500 transition-colors"
          >
            <Bookmark
              className={`w-3.5 h-3.5 ${post.isBookmarked ? "fill-amber-500 text-amber-500" : ""}`}
            />
          </button>
        </div>
      </div>
    </article>
  );
}
