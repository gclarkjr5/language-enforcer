<script>
  import { onMount, onDestroy } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'

  let current = null
  let showAnswer = false
  let loading = false
  let error = ''
  let dueCount = 0
  let totalCount = 0
  let sessionActive = false
  let showSessionPrompt = false
  let reviewedThisSession = 0
  let reportNote = ''
  let showReport = false

  const grades = [
    { label: 'Again', value: 1 },
    { label: 'Hard', value: 3 },
    { label: 'Good', value: 4 },
    { label: 'Easy', value: 5 }
  ]

  async function refreshCounts() {
    try {
      const [due, total] = await invoke('counts')
      dueCount = due
      totalCount = total
    } catch (err) {
      console.error(err)
    }
  }

  async function loadNext() {
    loading = true
    error = ''
    try {
      const next = await invoke('next_due_card')
      current = next
      showAnswer = false
      if (!next && sessionActive) {
        showSessionPrompt = true
      }
    } catch (err) {
      error = String(err)
    } finally {
      loading = false
    }
  }

  async function startSession() {
    loading = true
    error = ''
    showSessionPrompt = false
    reviewedThisSession = 0
    try {
      await invoke('start_session')
      sessionActive = true
      await refreshCounts()
      await loadNext()
    } catch (err) {
      error = String(err)
    } finally {
      loading = false
    }
  }

  async function grade(value) {
    if (!current) return
    loading = true
    error = ''
    try {
      await invoke('grade_card', { input: { card_id: current.card_id, grade: value } })
      reviewedThisSession += 1
      await refreshCounts()
      await loadNext()
    } catch (err) {
      error = String(err)
    } finally {
      loading = false
    }
  }

  function reveal() {
    showAnswer = true
  }

  function openReport() {
    reportNote = ''
    showReport = true
  }

  async function submitReport() {
    if (!current) return
    loading = true
    error = ''
    try {
      await invoke('report_issue', {
        input: {
          card_id: current.card_id,
          word_id: current.word_id,
          text: current.text,
          translation: current.translation ?? null,
          note: reportNote.trim() ? reportNote.trim() : null,
          reported_at: new Date().toISOString()
        }
      })
      showReport = false
    } catch (err) {
      error = String(err)
    } finally {
      loading = false
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

  onMount(async () => {
    window.addEventListener('keydown', handleKey)
    await refreshCounts()
    await startSession()
  })

  onDestroy(() => {
    window.removeEventListener('keydown', handleKey)
  })
</script>

<main>
  <header>
    <div>
      <h1>Language Enforcer Review</h1>
      <p class="meta">Due: {dueCount} / {totalCount}</p>
    </div>
    <button class="ghost" on:click={loadNext} disabled={loading}>Refresh</button>
  </header>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  {#if showSessionPrompt}
    <div class="modal-backdrop">
      <div class="modal">
        <h2>Session complete</h2>
        <p>You've finished 10 cards. Want another 10?</p>
        <div class="modal-actions">
          <button class="grade" on:click={startSession}>Another 10</button>
          <button class="ghost" on:click={() => { showSessionPrompt = false; sessionActive = false; }}>End session</button>
        </div>
      </div>
    </div>
  {/if}

  {#if showReport}
    <div class="modal-backdrop">
      <div class="modal">
        <h2>Report issue</h2>
        <p>What’s wrong with this card?</p>
        <textarea bind:value={reportNote} rows="4" placeholder="Optional note"></textarea>
        <div class="modal-actions">
          <button class="grade" on:click={submitReport} disabled={loading}>Submit</button>
          <button class="ghost" on:click={() => (showReport = false)} disabled={loading}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}

  {#if loading}
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
      <button class="report" on:click={openReport}>Report issue</button>
    </div>

    <div class="actions">
      {#each grades as grade}
        <button
          class="grade"
          disabled={!showAnswer || loading}
          on:click={() => grade(grade.value)}>
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
  main {
    max-width: 920px;
    margin: 0 auto;
    padding: 32px 24px 48px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
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
  .tagline {
    color: #94a3b8;
    font-size: 14px;
    margin-bottom: 16px;
  }
  .prompt {
    font-size: 36px;
    font-weight: 600;
    margin-bottom: 18px;
  }
  .answer {
    font-size: 28px;
    color: #38bdf8;
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
  textarea {
    width: 100%;
    margin-top: 12px;
    background: #0f172a;
    color: #e2e8f0;
    border: 1px solid #334155;
    border-radius: 10px;
    padding: 10px;
    resize: vertical;
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
</style>
