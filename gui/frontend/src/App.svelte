<script>
  import { onMount, onDestroy } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import {
    getAuthState,
    refreshAuthState,
    fetchDataApiSnapshot,
    updateWord,
    signInEmail,
    signUpEmail,
  } from './lib/auth.js'

  let current = null
  let showAnswer = false
  let loading = false
  let showLoadingCard = false
  let syncing = false
  let error = ''
  let dueCount = 0
  let totalCount = 0
  let sessionActive = false
  let showSessionPrompt = false
  let reviewedThisSession = 0
  let showFix = false
  let fixText = ''
  let fixTranslation = ''
  let fixAuthMessage = ''
  let fixAuthTimer = null
  let authState = getAuthState()
  let toastMessage = ''
  let toastTimer = null
  let email = ''
  let password = ''
  let name = ''
  let authMessage = ''
  let authMessageTimer = null
  let showAuthModal = false
  let authMode = 'signin'
  let unsubscribeDeepLink = null
  $: showError = Boolean(error) && !isAuthRequiredError(error)
  $: isBusy = loading || syncing

  function showToast(message) {
    toastMessage = message
    if (toastTimer) clearTimeout(toastTimer)
    toastTimer = setTimeout(() => {
      toastMessage = ''
      toastTimer = null
    }, 2000)
  }

  function showAuthMessage(message) {
    authMessage = message
    if (authMessageTimer) clearTimeout(authMessageTimer)
    authMessageTimer = setTimeout(() => {
      authMessage = ''
      authMessageTimer = null
    }, 2000)
  }

  function isAuthRequiredError(err) {
    if (!err) return false
    const message = typeof err === 'string' ? err : err.message
    if (!message) return false
    return message.toLowerCase().includes('must be signed in')
  }

  function logAuth(message, data) {
    if (data === undefined) {
      console.info(`[auth] ${message}`)
      return
    }
    console.info(`[auth] ${message}`, data)
  }

  function openAuthModal(mode) {
    authMode = mode
    authMessage = ''
    if (authMessageTimer) {
      clearTimeout(authMessageTimer)
      authMessageTimer = null
    }
    showAuthModal = true
  }

  function closeAuthModal() {
    showAuthModal = false
  }

  function handleBackdropKey(event, onClose) {
    if (event.key === 'Escape' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      onClose()
    }
  }

  function handleDeepLink(url) {
    try {
      const parsed = new URL(url)
      const verifier = parsed.searchParams.get('neon_auth_session_verifier')
      if (!verifier) return
      const current = new URL(window.location.href)
      current.searchParams.set('neon_auth_session_verifier', verifier)
      window.history.replaceState(window.history.state, '', current.href)
      refreshAuthState().then(() => {
        authState = getAuthState()
        showToast('Signed in')
      })
    } catch (err) {
      error = String(err)
    }
  }

  const grades = [
    { label: 'Again', value: 1 },
    { label: 'Hard', value: 3 },
    { label: 'Good', value: 4 },
    { label: 'Easy', value: 5 }
  ]

  const isTauri =
    typeof window !== 'undefined' &&
    (Boolean(window.__TAURI__) || Boolean(window.__TAURI_INTERNALS__))

  async function refreshCounts() {
    try {
      if (!isTauri) return
      const [due, total] = await invoke('counts')
      dueCount = due
      totalCount = total
    } catch (err) {
      console.error(err)
    }
  }

  async function loadNext({ silent = false } = {}) {
    if (!silent) {
      loading = true
      showLoadingCard = true
    }
    error = ''
    try {
      if (!isTauri) return
      const next = await invoke('next_due_card')
      current = next
      showAnswer = false
      if (!next && sessionActive && reviewedThisSession > 0) {
        showSessionPrompt = true
      }
    } catch (err) {
      if (isAuthRequiredError(err)) {
        showToast('Must be signed in to use this feature')
        error = ''
      } else {
        error = String(err)
      }
    } finally {
      if (!silent) {
        loading = false
        showLoadingCard = false
      }
    }
  }

  async function startSession() {
    loading = true
    error = ''
    showSessionPrompt = false
    reviewedThisSession = 0
    try {
      if (!isTauri) return
      await invoke('start_session')
      sessionActive = true
      await refreshCounts()
      await loadNext({ silent: true })
    } catch (err) {
      if (isAuthRequiredError(err)) {
        showToast('Must be signed in to use this feature')
        error = ''
      } else {
        error = String(err)
      }
    } finally {
      loading = false
    }
  }

  async function grade(value) {
    if (!current) return
    loading = true
    error = ''
    try {
      if (!isTauri) return
      await invoke('grade_card', { input: { card_id: current.card_id, grade: value } })
      reviewedThisSession += 1
      await refreshCounts()
      await loadNext({ silent: true })
    } catch (err) {
      error = String(err)
    } finally {
      loading = false
    }
  }

  function handleGradeTap(event, value) {
    event.preventDefault()
    grade(value)
  }

  function reveal() {
    showAnswer = true
  }

  function openFix() {
    if (!current) return
    fixText = current.text ?? ''
    fixTranslation = current.translation ?? ''
    fixAuthMessage = ''
    if (fixAuthTimer) {
      clearTimeout(fixAuthTimer)
      fixAuthTimer = null
    }
    showFix = true
  }

  async function submitFix() {
    if (!current) return
    loading = true
    error = ''
    await refreshAuthState()
    authState = getAuthState()
    if (authState !== 'signed_in') {
      fixAuthMessage = 'Must be signed in to use this feature'
      if (fixAuthTimer) clearTimeout(fixAuthTimer)
      fixAuthTimer = setTimeout(() => {
        fixAuthMessage = ''
        fixAuthTimer = null
      }, 2000)
      loading = false
      return
    }
    fixAuthMessage = ''
    const nextText = fixText.trim()
    const nextTranslation = fixTranslation.trim()
    const textChanged = nextText !== current.text
    const translationChanged = nextTranslation !== (current.translation ?? '')
    if (!textChanged && !translationChanged) {
      showFix = false
      loading = false
      return
    }
    try {
      await updateWord({
        wordId: current.word_id,
        text: textChanged ? nextText : null,
        translation: translationChanged ? nextTranslation : null
      })
      if (isTauri) {
        await invoke('apply_correction_local', {
          input: {
            word_id: current.word_id,
            text: textChanged ? nextText : null,
            translation: translationChanged ? nextTranslation : null
          }
        })
      }
      if (textChanged) current.text = nextText
      if (translationChanged) current.translation = nextTranslation
      showFix = false
    } catch (err) {
      error = String(err)
    } finally {
      loading = false
    }
  }

  async function syncFromPostgres() {
    syncing = true
    error = ''
    showSessionPrompt = false
    try {
      if (!isTauri) {
        throw new Error('Sync is only available in the desktop app.')
      }
      logAuth('refresh data: session', await refreshAuthState())
      authState = getAuthState()
      if (authState !== 'signed_in') {
        showToast('Must be signed in to use this feature')
        return
      }
      showToast('Refreshing data...')
      const snapshot = await fetchDataApiSnapshot()
      await invoke('refresh_from_data_api', { snapshot })
      showToast('Data refreshed')
      reviewedThisSession = 0
      sessionActive = true
      await invoke('start_session')
      await refreshCounts()
      await loadNext()
    } catch (err) {
      error = String(err)
    } finally {
      syncing = false
    }
  }

  function handleKey(event) {
    if (!current) return
    if (!showAnswer && (event.key === ' ' || event.key === 'Enter')) {
      event.preventDefault()
      showAnswer = true
      return
    }
    if (showAnswer) {
      if (event.key === '1') return grade(1)
      if (event.key === '2') return grade(3)
      if (event.key === '3') return grade(4)
      if (event.key === '4') return grade(5)
    }
  }

  async function beginSignIn() {
    error = ''
    try {
      const session = await refreshAuthState()
      authState = getAuthState()
      if (session) {
        showToast('Already signed in')
        return
      }
      if (!email.trim() || !password.trim()) {
        showAuthMessage('Email and password are required.')
        return
      }
      authMessage = ''
      logAuth('sign-in: starting', { email: email.trim() })
      await signInEmail(email.trim(), password)
      console.info('[auth] sign-in response', window.__leAuthLastSignIn ?? null)
      authState = getAuthState()
      logAuth('sign-in: session', await refreshAuthState())
      showToast('Signed in')
      closeAuthModal()
    } catch (err) {
      logAuth('sign-in error', String(err))
      showAuthMessage(String(err))
    }
  }

  async function beginSignUp() {
    error = ''
    try {
      if (!email.trim() || !password.trim()) {
        showAuthMessage('Email and password are required.')
        return
      }
      authMessage = ''
      logAuth('sign-up: starting', { email: email.trim() })
      await signUpEmail(email.trim(), password, name.trim() || undefined)
      console.info('[auth] sign-up response', window.__leAuthLastSignUp ?? null)
      authState = getAuthState()
      logAuth('sign-up: session', await refreshAuthState())
      showToast('Signed up')
      closeAuthModal()
    } catch (err) {
      logAuth('sign-up error', String(err))
      showAuthMessage('Error signing up')
    }
  }

  onMount(async () => {
    window.addEventListener('keydown', handleKey)
    await refreshCounts()
    await startSession()
    try {
      await refreshAuthState()
      authState = getAuthState()
      if (isTauri) {
        unsubscribeDeepLink = await listen('deep-link', (event) => {
          const payload = event.payload
          if (Array.isArray(payload)) {
            payload.forEach((url) => handleDeepLink(String(url)))
          } else if (payload) {
            handleDeepLink(String(payload))
          }
        })
      }
    } catch (err) {
      error = String(err)
    }
  })

  onDestroy(() => {
    window.removeEventListener('keydown', handleKey)
    if (unsubscribeDeepLink) unsubscribeDeepLink()
  })
</script>

<main>
  <header>
    <div class="header-left">
      <button class="ghost" on:click={() => openAuthModal('signin')} disabled={isBusy}>Sign in</button>
      <button class="ghost" on:click={() => openAuthModal('signup')} disabled={isBusy}>Sign up</button>
    </div>
    <div class="header-center">
      {#if toastMessage}
        <div class="toast">{toastMessage}</div>
      {/if}
      <h1>Language Enforcer</h1>
      <p class="meta">Due: {dueCount} / {totalCount}</p>
    </div>
    <div class="header-actions">
      <button class="ghost" on:click={syncFromPostgres} disabled={isBusy}>Refresh Data</button>
    </div>
  </header>

  {#if showError}
    <div class="error">{error}</div>
  {/if}

  {#if showSessionPrompt}
    <div
      class="modal-backdrop"
      role="button"
      tabindex="0"
      aria-label="Close session prompt"
      on:click={() => { showSessionPrompt = false; sessionActive = false; }}
      on:keydown={(event) => handleBackdropKey(event, () => { showSessionPrompt = false; sessionActive = false; })}>
      <div
        class="modal"
        role="dialog"
        aria-modal="true"
        tabindex="0"
        on:click|stopPropagation
        on:keydown|stopPropagation>
        <h2>Session complete</h2>
        <p>You've finished 10 cards. Want another 10?</p>
        <div class="modal-actions">
          <button class="grade" on:click={startSession}>Another 10</button>
          <button class="ghost" on:click={() => { showSessionPrompt = false; sessionActive = false; }}>End session</button>
        </div>
      </div>
    </div>
  {/if}

  {#if showAuthModal}
    <div
      class="modal-backdrop"
      role="button"
      tabindex="0"
      aria-label="Close sign in dialog"
      on:click={closeAuthModal}
      on:keydown={(event) => handleBackdropKey(event, closeAuthModal)}>
      <div
        class="modal"
        role="dialog"
        aria-modal="true"
        tabindex="0"
        on:click|stopPropagation
        on:keydown|stopPropagation>
        <h2>{authMode === 'signup' ? 'Create account' : 'Sign in'}</h2>
        {#if authMessage}
          <div class="modal-note">{authMessage}</div>
        {/if}
        <label class="auth-field">
          <span>Email</span>
          <input type="email" bind:value={email} placeholder="you@example.com" />
        </label>
        <label class="auth-field">
          <span>Password</span>
          <input type="password" bind:value={password} placeholder="••••••••" />
        </label>
        {#if authMode === 'signup'}
          <label class="auth-field">
            <span>Name</span>
            <input type="text" bind:value={name} placeholder="Your name" />
          </label>
        {/if}
        <div class="auth-actions">
          {#if authMode === 'signup'}
            <button class="grade" on:click={beginSignUp} disabled={isBusy}>Sign up</button>
          {:else}
            <button class="grade" on:click={beginSignIn} disabled={isBusy}>Sign in</button>
          {/if}
          <button class="ghost" on:click={closeAuthModal} disabled={isBusy}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}

  {#if showFix}
    <div
      class="modal-backdrop"
      role="button"
      tabindex="0"
      aria-label="Close fix card dialog"
      on:click={() => (showFix = false)}
      on:keydown={(event) => handleBackdropKey(event, () => (showFix = false))}>
      <div
        class="modal"
        role="dialog"
        aria-modal="true"
        tabindex="0"
        on:click|stopPropagation
        on:keydown|stopPropagation>
        <h2>Fix card</h2>
        {#if fixAuthMessage}
          <div class="modal-note">{fixAuthMessage}</div>
        {/if}
        <label class="field">
          <span>Dutch</span>
          <input bind:value={fixText} placeholder="Dutch word" />
        </label>
        <label class="field">
          <span>English</span>
          <input bind:value={fixTranslation} placeholder="English translation" />
        </label>
        <div class="modal-actions">
          <button class="grade" on:click={submitFix} disabled={isBusy}>Save</button>
          <button class="ghost" on:click={() => (showFix = false)} disabled={isBusy}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}

  {#if showLoadingCard}
    <div class="card">Loading…</div>
  {:else if !current}
    <div class="card empty">
      <h2>No cards due</h2>
      <p>Add more words in the TUI or wait for cards to become due.</p>
    </div>
  {:else}
    <div class="card">
      <div class="tagline">{current.chapter ?? 'Unassigned'} • {current.group ?? 'Ungrouped'}</div>
      <div class="prompt">{current.text}</div>
      {#if showAnswer}
        <div class="answer">{current.translation ?? '—'}</div>
      {:else}
        <button class="reveal" on:click={reveal}>Show answer</button>
      {/if}
      <button class="report" on:click={openFix}>Fix text</button>
    </div>

    <div class="actions">
      {#each grades as grade}
        <button
          class="grade"
          disabled={!showAnswer || isBusy}
          on:click={(event) => handleGradeTap(event, grade.value)}
          on:touchend={(event) => handleGradeTap(event, grade.value)}>
          <span>{grade.label}</span>
          <small>{grade.value === 5 ? '4' : grade.value === 4 ? '3' : grade.value === 3 ? '2' : '1'}</small>
        </button>
      {/each}
    </div>

    <div class="hint">Space/Enter to reveal. 1–4 to grade. Session: {reviewedThisSession}/10</div>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: "Inter", "Helvetica Neue", Arial, sans-serif;
    background: #0f172a;
    color: #e2e8f0;
  }
  :global(*), :global(*::before), :global(*::after) {
    box-sizing: border-box;
  }
  main {
    width: 100%;
    max-width: 920px;
    margin: 0 auto;
    padding: 32px 20px 48px;
  }
  header {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    margin-bottom: 24px;
  }
  .header-left {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  .auth-field {
    width: 220px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .auth-field span {
    color: #94a3b8;
    font-size: 11px;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }
  .auth-actions {
    display: flex;
    gap: 8px;
  }
  .toast {
    font-size: 12px;
    color: #93c5fd;
    margin-bottom: 4px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .header-center {
    text-align: center;
  }
  .header-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
  }
  h1 {
    margin: 0 0 6px;
    font-size: 28px;
  }
  .meta {
    margin: 0;
    color: #94a3b8;
  }
  .ghost {
    border: 1px solid #334155;
    background: transparent;
    color: #cbd5f5;
    padding: 8px 14px;
    border-radius: 8px;
    cursor: pointer;
  }
  .card {
    background: #111827;
    border-radius: 16px;
    padding: 32px;
    box-shadow: 0 20px 40px rgba(15, 23, 42, 0.4);
  }
  .card.empty {
    text-align: center;
  }
  @media (max-width: 680px) {
    main {
      padding: 24px 16px 40px;
    }
    header {
      grid-template-columns: 1fr;
      text-align: center;
    }
    .header-left,
    .header-actions {
      justify-content: center;
      flex-wrap: wrap;
    }
  }
  .tagline {
    color: #94a3b8;
    font-size: 14px;
    margin-bottom: 16px;
  }
  .prompt {
    font-size: clamp(22px, 4.5vw, 36px);
    font-weight: 600;
    margin-bottom: 18px;
    overflow-wrap: anywhere;
    word-break: break-word;
  }
  .answer {
    font-size: clamp(18px, 3.6vw, 28px);
    color: #38bdf8;
    overflow-wrap: anywhere;
    word-break: break-word;
  }
  .reveal {
    background: #2563eb;
    border: none;
    color: #fff;
    padding: 10px 18px;
    border-radius: 10px;
    cursor: pointer;
  }
  .actions {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: 12px;
    margin-top: 20px;
  }
  .report {
    margin-top: 16px;
    background: transparent;
    border: 1px dashed #475569;
    color: #cbd5f5;
    padding: 8px 12px;
    border-radius: 10px;
    cursor: pointer;
  }
  input {
    width: 100%;
    background: #0f172a;
    color: #e2e8f0;
    border: 1px solid #334155;
    border-radius: 10px;
    padding: 10px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
  }
  .field span {
    color: #94a3b8;
    font-size: 12px;
    letter-spacing: 0.02em;
  }
  .grade {
    background: #1f2937;
    border: 1px solid #334155;
    color: #e2e8f0;
    padding: 10px 12px;
    border-radius: 12px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    touch-action: manipulation;
    -webkit-tap-highlight-color: transparent;
  }
  .grade:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .grade small {
    color: #94a3b8;
    font-size: 12px;
  }
  .hint {
    margin-top: 14px;
    color: #94a3b8;
    font-size: 13px;
  }
  .error {
    background: #7f1d1d;
    color: #fee2e2;
    padding: 12px 16px;
    border-radius: 10px;
    margin-bottom: 16px;
  }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(15, 23, 42, 0.72);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 20;
  }
  .modal {
    background: #111827;
    padding: 24px;
    border-radius: 16px;
    min-width: 280px;
    box-shadow: 0 20px 40px rgba(15, 23, 42, 0.45);
  }
  .modal-actions {
    margin-top: 16px;
    display: flex;
    gap: 12px;
  }
  .modal-note {
    margin: 8px 0 12px;
    padding: 8px 10px;
    border-radius: 8px;
    background: #1e293b;
    color: #cbd5f5;
    font-size: 12px;
  }
</style>
