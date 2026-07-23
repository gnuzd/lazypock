<script>
  import { store } from "../lib/store.svelte.js";
  import { api } from "../lib/api.js";

  let { onlogin = () => {} } = $props();

  let email = $state("");
  let password = $state("");
  let error = $state("");
  let loading = $state(false);

  async function handleSubmit(e) {
    e.preventDefault();
    loading = true;
    error = "";
    try {
      await api.login(email, password);
      store.superuser = await api.me();
      await store.loadCollections();
      onlogin();
    } catch (err) {
      error = err.message || "Login failed";
    } finally {
      loading = false;
    }
  }
</script>

<div class="login-page">
  <form class="login-form" onsubmit={handleSubmit}>
    <div class="login-logo">
      <img src="/_/images/logo.svg" alt="Lazypock" onerror={(e) => e.target.style.display = 'none'} />
      <h1>Lazypock</h1>
    </div>

    {#if error}
      <div class="toast toast-error" style="margin-bottom:12px">
        <span>{error}</span>
      </div>
    {/if}

    <div class="form-field">
      <label for="email">Email</label>
      <input id="email" type="email" class="input" placeholder="superuser@example.com" bind:value={email} required />
    </div>

    <div class="form-field">
      <label for="password">Password</label>
      <input id="password" type="password" class="input" placeholder="password" bind:value={password} required />
    </div>

    <button type="submit" class="btn btn-primary" style="width:100%" disabled={loading}>
      {loading ? "Signing in..." : "Sign in"}
    </button>
  </form>
</div>

<style>
  .login-page {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    background: var(--color-primary);
  }
  .login-form {
    width: 100%;
    max-width: 340px;
    padding: var(--spacing);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .login-logo {
    text-align: center;
    margin-bottom: 12px;
  }
  .login-logo h1 {
    font-size: 22px;
    font-weight: 600;
    color: var(--color-primary-content);
    margin-top: 10px;
  }
  .login-logo img {
    height: 55px;
    margin: 0 auto;
  }
</style>
