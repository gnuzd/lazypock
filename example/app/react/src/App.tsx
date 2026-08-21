import { useEffect, useState } from "react";
import AuthModal from "./components/Auth";
import CreatePost from "./components/CreatePost";
import Navbar from "./components/Navbar";
import PostCard from "./components/PostCard";
import PostDetail from "./components/PostDetail";
import Toast from "./components/Toast";
import { client } from "./lib/client";

export default function App() {
  const [posts, setPosts] = useState([]);
  const [activeCategory, setActiveCategory] = useState("All");
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedPostId, setSelectedPostId] = useState(null);
  const [isCreatingPost, setIsCreatingPost] = useState(false);
  const [toastMessage, setToastMessage] = useState(null);

  // Authentication State
  const [currentUser, setCurrentUser] = useState(null); // null = Guest
  const [isAuthModalOpen, setIsAuthModalOpen] = useState(false);
  const [authModalMode, setAuthModalMode] = useState("login");

  useEffect(() => {
    const init = async () => {
      await client.authStore.init();

      if (client.authStore.isValid) {
        setCurrentUser(client.authStore.model);
      }

      client.authStore.onChange((user) => setCurrentUser(user));
    };

    init();
  }, []);

  const showNotification = (msg) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  const handleOpenAuth = (mode = "login") => {
    setAuthModalMode(mode);
    setIsAuthModalOpen(true);
  };

  const handleLoginSuccess = (userObj, notificationText) => {
    setCurrentUser(userObj);
    showNotification(notificationText);
  };

  const handleLogout = () => {
    setCurrentUser(null);
    showNotification("Signed out successfully.");
  };

  const handleStartCreatePost = () => {
    if (!currentUser) {
      handleOpenAuth("login");
      return;
    }
    setIsCreatingPost(true);
    setSelectedPostId(null);
  };

  // Filter posts
  const filteredPosts = posts.filter((post) => {
    const matchesCategory =
      activeCategory === "All" || post.category === activeCategory;
    const matchesSearch =
      post.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
      post.excerpt.toLowerCase().includes(searchQuery.toLowerCase()) ||
      post.author.name.toLowerCase().includes(searchQuery.toLowerCase());
    return matchesCategory && matchesSearch;
  });

  const selectedPost = posts.find((p) => p.id === selectedPostId);

  // Actions
  const handleToggleLike = (postId, e) => {
    if (e) e.stopPropagation();
    setPosts((prev) =>
      prev.map((post) => {
        if (post.id === postId) {
          const isLiked = !post.isLiked;
          return {
            ...post,
            isLiked,
            likes: isLiked ? post.likes + 1 : post.likes - 1,
          };
        }
        return post;
      }),
    );
  };

  const handleToggleBookmark = (postId, e) => {
    if (e) e.stopPropagation();
    setPosts((prev) =>
      prev.map((post) => {
        if (post.id === postId) {
          const isBookmarked = !post.isBookmarked;
          showNotification(
            isBookmarked ? "Saved to bookmarks" : "Removed from bookmarks",
          );
          return { ...post, isBookmarked };
        }
        return post;
      }),
    );
  };

  const handleAddComment = (commentText) => {
    if (!selectedPostId) return;

    const authorName = currentUser ? currentUser.name : "Guest Reader";
    const authorAvatar = currentUser
      ? currentUser.avatar
      : `https://api.dicebear.com/7.x/bottts/svg?seed=Guest`;

    const newComment = {
      id: "c_" + Date.now(),
      author: authorName,
      avatar: authorAvatar,
      date: "Just now",
      text: commentText,
    };

    setPosts((prev) =>
      prev.map((p) => {
        if (p.id === selectedPostId) {
          return {
            ...p,
            comments: [newComment, ...p.comments],
          };
        }
        return p;
      }),
    );

    showNotification("Comment posted!");
  };

  const handleCreatePostSubmit = ({ title, category, excerpt, content }) => {
    const createdPost = {
      id: "p_" + Date.now(),
      title,
      category,
      excerpt: excerpt || content.slice(0, 120) + "...",
      content,
      author: {
        name: currentUser.name,
        role: currentUser.role,
        avatar: currentUser.avatar,
      },
      date: "Just now",
      readTime: `${Math.max(1, Math.ceil(content.split(" ").length / 150))} min read`,
      coverImage:
        "https://images.unsplash.com/photo-1499750310107-5fef28a66643?w=1200&auto=format&fit=crop&q=80",
      likes: 0,
      isLiked: false,
      isBookmarked: false,
      comments: [],
    };

    setPosts([createdPost, ...posts]);
    setIsCreatingPost(false);
    setSelectedPostId(createdPost.id);
    showNotification("Article published successfully!");
  };

  return (
    <div className="min-h-screen bg-slate-50 text-slate-800 font-sans pb-16">
      <Toast message={toastMessage} />

      <AuthModal
        isOpen={isAuthModalOpen}
        onClose={() => setIsAuthModalOpen(false)}
        initialMode={authModalMode}
        onLoginSuccess={handleLoginSuccess}
      />

      <Navbar
        currentUser={currentUser}
        onOpenAuth={handleOpenAuth}
        onGoHome={() => {
          setSelectedPostId(null);
          setIsCreatingPost(false);
        }}
        onStartCreatePost={handleStartCreatePost}
        searchQuery={searchQuery}
        setSearchQuery={setSearchQuery}
        isCreatingPost={isCreatingPost}
        selectedPostId={selectedPostId}
        onLogout={handleLogout}
      />

      <main className="max-w-6xl mx-auto px-4 sm:px-6 pt-8">
        {/* VIEW 1: CREATE POST */}
        {isCreatingPost && (
          <CreatePost
            currentUser={
              currentUser || { name: "Author", role: "Writer", avatar: "" }
            }
            categories={[]}
            onCancel={() => setIsCreatingPost(false)}
            onSubmit={handleCreatePostSubmit}
          />
        )}

        {/* VIEW 2: POST DETAIL */}
        {!isCreatingPost && selectedPost && (
          <PostDetail
            post={selectedPost}
            onBack={() => setSelectedPostId(null)}
            onToggleLike={handleToggleLike}
            onToggleBookmark={handleToggleBookmark}
            onShowNotification={showNotification}
            currentUser={currentUser}
            onAddComment={handleAddComment}
            onOpenAuth={handleOpenAuth}
          />
        )}

        {/* VIEW 3: HOME FEED */}
        {!isCreatingPost && !selectedPost && (
          <div className="space-y-8 animate-fadeIn">
            {/* <HeroBanner /> */}

            {/* <CategoryFilter */}
            {/*   categories={CATEGORIES} */}
            {/*   activeCategory={activeCategory} */}
            {/*   onSelectCategory={setActiveCategory} */}
            {/*   totalResults={filteredPosts.length} */}
            {/* /> */}

            {filteredPosts.length === 0 ? (
              <div className="bg-white rounded-3xl border border-slate-200 p-12 text-center space-y-3">
                <p className="text-slate-500 font-medium text-xs sm:text-sm">
                  No articles matched your criteria.
                </p>
                <button
                  onClick={() => {
                    setActiveCategory("All");
                    setSearchQuery("");
                  }}
                  className="text-xs font-bold text-indigo-600 hover:underline"
                >
                  Clear all filters
                </button>
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                {filteredPosts.map((post) => (
                  <PostCard
                    key={post.id}
                    post={post}
                    onSelect={setSelectedPostId}
                    onToggleLike={handleToggleLike}
                    onToggleBookmark={handleToggleBookmark}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </main>
    </div>
  );
}
