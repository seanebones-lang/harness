/**
 * Harness VS Code Extension
 * 
 * Connects to the Harness daemon via Unix socket (or HTTP fallback),
 * provides a side-panel chat webview, Cmd+I inline edit, and status bar.
 */

import * as vscode from 'vscode';
import * as net from 'net';
import * as path from 'path';
import * as os from 'os';

// ── Types ────────────────────────────────────────────────────────────────────

interface HarnessEvent {
  type: 'text_chunk' | 'tool_start' | 'tool_result' | 'done' | 'error' | 'token_usage';
  content?: string;
  name?: string;
  result?: string;
  message?: string;
  input?: number;
  output?: number;
}

// ── Connection ────────────────────────────────────────────────────────────────

class HarnessDaemonClient {
  private socket: net.Socket | null = null;
  private socketPath: string;
  private connected = false;

  constructor(socketPath: string) {
    this.socketPath = socketPath.replace('~', os.homedir());
  }

  async connect(): Promise<boolean> {
    return new Promise((resolve) => {
      this.socket = net.createConnection({ path: this.socketPath }, () => {
        this.connected = true;
        resolve(true);
      });
      this.socket.on('error', () => {
        this.connected = false;
        resolve(false);
      });
    });
  }

  isConnected(): boolean {
    return this.connected;
  }

  disconnect() {
    this.socket?.destroy();
    this.connected = false;
  }

  async sendPrompt(
    prompt: string,
    sessionId: string | undefined,
    onEvent: (event: HarnessEvent) => void
  ): Promise<void> {
    if (!this.socket || !this.connected) {
      throw new Error('Not connected to daemon');
    }

    const request = JSON.stringify({
      action: 'chat',
      prompt,
      session_id: sessionId,
    }) + '\n';

    this.socket.write(request);

    return new Promise((resolve, reject) => {
      let buf = '';
      const handler = (data: Buffer) => {
        buf += data.toString();
        const lines = buf.split('\n');
        buf = lines.pop() || '';
        for (const line of lines) {
          if (!line.trim()) continue;
          try {
            const event: HarnessEvent = JSON.parse(line);
            onEvent(event);
            if (event.type === 'done' || event.type === 'error') {
              this.socket?.off('data', handler);
              resolve();
            }
          } catch { /* ignore parse errors */ }
        }
      };
      this.socket!.on('data', handler);
      this.socket!.once('error', reject);
    });
  }
}

// ── Chat Panel ────────────────────────────────────────────────────────────────

class HarnessChatPanel {
  private panel: vscode.WebviewPanel | null = null;
  private client: HarnessDaemonClient;
  private sessionId: string | undefined;
  private statusBar: vscode.StatusBarItem;

  constructor(client: HarnessDaemonClient, statusBar: vscode.StatusBarItem) {
    this.client = client;
    this.statusBar = statusBar;
  }

  show(context: vscode.ExtensionContext) {
    if (this.panel) {
      this.panel.reveal(vscode.ViewColumn.Beside);
      return;
    }

    this.panel = vscode.window.createWebviewPanel(
      'harnessChat',
      'Harness Chat',
      vscode.ViewColumn.Beside,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, 'media')],
      }
    );

    this.panel.webview.html = this.getHtml();

    this.panel.webview.onDidReceiveMessage(async (msg) => {
      if (msg.type === 'send') {
        await this.sendMessage(msg.text);
      }
    });

    this.panel.onDidDispose(() => {
      this.panel = null;
    });
  }

  private async sendMessage(text: string) {
    if (!this.panel) return;

    this.panel.webview.postMessage({ type: 'user', text });
    this.panel.webview.postMessage({ type: 'assistant_start' });
    this.statusBar.text = '$(sync~spin) Harness: thinking…';

    let fullText = '';
    try {
      await this.client.sendPrompt(text, this.sessionId, (event) => {
        if (!this.panel) return;
        switch (event.type) {
          case 'text_chunk':
            fullText += event.content || '';
            this.panel.webview.postMessage({ type: 'chunk', text: event.content || '' });
            break;
          case 'tool_start':
            this.panel.webview.postMessage({ type: 'tool', name: `→ ${event.name}` });
            this.statusBar.text = `$(sync~spin) ${event.name}…`;
            break;
          case 'done':
            this.panel.webview.postMessage({ type: 'done' });
            this.statusBar.text = '$(sparkle) Harness';
            break;
          case 'error':
            this.panel.webview.postMessage({ type: 'error', text: event.message || '' });
            this.statusBar.text = '$(error) Harness: error';
            break;
        }
      });
    } catch (e) {
      this.panel?.webview.postMessage({ type: 'error', text: String(e) });
      this.statusBar.text = '$(error) Harness: disconnected';
    }
  }

  private getHtml(): string {
    return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  body { font-family: var(--vscode-font-family); font-size: 13px; padding: 12px; background: var(--vscode-editor-background); color: var(--vscode-editor-foreground); margin: 0; }
  #messages { height: calc(100vh - 80px); overflow-y: auto; padding-bottom: 8px; }
  .msg { margin: 8px 0; padding: 8px 12px; border-radius: 6px; line-height: 1.5; }
  .user { background: var(--vscode-inputValidation-infoBackground); text-align: right; }
  .assistant { background: var(--vscode-editor-inactiveSelectionBackground); white-space: pre-wrap; font-family: var(--vscode-editor-font-family); }
  .tool { color: var(--vscode-descriptionForeground); font-size: 11px; padding: 2px 8px; }
  .error { color: var(--vscode-errorForeground); }
  #input-area { position: fixed; bottom: 0; left: 0; right: 0; padding: 8px; background: var(--vscode-editor-background); display: flex; gap: 8px; }
  #input { flex: 1; background: var(--vscode-input-background); color: var(--vscode-input-foreground); border: 1px solid var(--vscode-input-border); padding: 6px 10px; border-radius: 4px; resize: none; height: 36px; font-family: inherit; }
  #send { background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; padding: 6px 16px; border-radius: 4px; cursor: pointer; }
  #send:hover { background: var(--vscode-button-hoverBackground); }
  .spinner { display: inline-block; animation: spin 1s linear infinite; }
  @keyframes spin { 100% { transform: rotate(360deg); } }
  code { font-family: var(--vscode-editor-font-family); background: var(--vscode-textCodeBlock-background); padding: 1px 4px; border-radius: 3px; }
  pre { background: var(--vscode-textCodeBlock-background); padding: 8px; border-radius: 4px; overflow-x: auto; }
</style>
</head>
<body>
<div id="messages"></div>
<div id="input-area">
  <textarea id="input" placeholder="Ask anything… (Enter to send, Shift+Enter for newline)"></textarea>
  <button id="send">Send</button>
</div>
<script>
const vscode = acquireVsCodeApi();
const messages = document.getElementById('messages');
const input = document.getElementById('input');
const sendBtn = document.getElementById('send');
let currentAssistantMsg = null;

function addMessage(html, cls) {
  const div = document.createElement('div');
  div.className = 'msg ' + cls;
  div.innerHTML = html;
  messages.appendChild(div);
  messages.scrollTop = messages.scrollHeight;
  return div;
}

function escapeHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function renderMarkdown(s) {
  // Simple inline markdown rendering
  return escapeHtml(s)
    .replace(/\`\`\`([\\s\\S]*?)\`\`\`/g, '<pre><code>$1</code></pre>')
    .replace(/\`([^\`]+)\`/g, '<code>$1</code>')
    .replace(/\\*\\*([^\\*]+)\\*\\*/g, '<strong>$1</strong>')
    .replace(/\\n/g, '<br>');
}

sendBtn.onclick = send;
input.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
});

function send() {
  const text = input.value.trim();
  if (!text) return;
  input.value = '';
  vscode.postMessage({ type: 'send', text });
}

window.addEventListener('message', (e) => {
  const msg = e.data;
  switch (msg.type) {
    case 'user':
      addMessage(escapeHtml(msg.text), 'user');
      break;
    case 'assistant_start':
      currentAssistantMsg = addMessage('<span class="spinner">⣾</span>', 'assistant');
      break;
    case 'chunk':
      if (currentAssistantMsg) {
        const raw = currentAssistantMsg.dataset.raw || '';
        currentAssistantMsg.dataset.raw = raw + msg.text;
        currentAssistantMsg.innerHTML = renderMarkdown(currentAssistantMsg.dataset.raw);
        messages.scrollTop = messages.scrollHeight;
      }
      break;
    case 'tool':
      addMessage(escapeHtml(msg.name), 'tool');
      break;
    case 'done':
      currentAssistantMsg = null;
      break;
    case 'error':
      if (currentAssistantMsg) {
        currentAssistantMsg.innerHTML = '<span class="error">Error: ' + escapeHtml(msg.text) + '</span>';
        currentAssistantMsg = null;
      } else {
        addMessage('<span class="error">Error: ' + escapeHtml(msg.text) + '</span>', 'error');
      }
      break;
  }
});
</script>
</body>
</html>`;
  }
}

// ── Inline Edit ───────────────────────────────────────────────────────────────

async function inlineEdit(
  editor: vscode.TextEditor,
  client: HarnessDaemonClient,
  statusBar: vscode.StatusBarItem
) {
  const selection = editor.selection;
  const selectedText = editor.document.getText(selection);
  if (!selectedText) {
    vscode.window.showInformationMessage('Select code to edit with Harness.');
    return;
  }

  const instruction = await vscode.window.showInputBox({
    prompt: 'Harness: How should I edit this code?',
    placeHolder: 'e.g. Add error handling, refactor to async, add JSDoc…',
  });

  if (!instruction) return;

  const lang = editor.document.languageId;
  const prompt = `Edit the following ${lang} code as instructed. Return ONLY the edited code, no explanation.\n\nInstruction: ${instruction}\n\nCode:\n\`\`\`${lang}\n${selectedText}\n\`\`\``;

  statusBar.text = '$(sync~spin) Harness: editing…';
  let fullResponse = '';

  try {
    await client.sendPrompt(prompt, undefined, (event) => {
      if (event.type === 'text_chunk') {
        fullResponse += event.content || '';
      }
    });

    // Extract code block if present
    const codeMatch = fullResponse.match(/```(?:\w+)?\n([\s\S]*?)```/);
    const newCode = codeMatch ? codeMatch[1].trimEnd() : fullResponse.trim();

    await editor.edit((eb) => {
      eb.replace(selection, newCode);
    });

    statusBar.text = '$(sparkle) Harness';
    vscode.window.showInformationMessage('Harness: edit applied.');
  } catch (e) {
    statusBar.text = '$(error) Harness: error';
    vscode.window.showErrorMessage(`Harness: ${e}`);
  }
}

// ── Activation ────────────────────────────────────────────────────────────────

export async function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration('harness');
  const socketPath = config.get<string>('daemonSocket', '~/.harness/daemon.sock');

  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusBar.text = '$(sparkle) Harness';
  statusBar.command = 'harness.openChat';
  statusBar.tooltip = 'Harness AI — click to open chat';
  statusBar.show();
  context.subscriptions.push(statusBar);

  const client = new HarnessDaemonClient(socketPath);

  // Auto-connect
  if (config.get<boolean>('autoConnect', true)) {
    const connected = await client.connect();
    statusBar.text = connected ? '$(sparkle) Harness' : '$(warning) Harness (disconnected)';
    if (!connected) {
      vscode.window.showWarningMessage(
        'Harness: daemon not running. Start with `harness daemon`.',
        'Start Daemon'
      ).then((choice) => {
        if (choice === 'Start Daemon') {
          vscode.window.createTerminal('Harness Daemon').sendText('harness daemon');
        }
      });
    }
  }

  const chatPanel = new HarnessChatPanel(client, statusBar);

  context.subscriptions.push(
    vscode.commands.registerCommand('harness.openChat', () => {
      chatPanel.show(context);
    }),

    vscode.commands.registerCommand('harness.inlineEdit', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      if (!client.isConnected()) {
        vscode.window.showErrorMessage('Harness: not connected to daemon. Run `harness daemon`.');
        return;
      }
      await inlineEdit(editor, client, statusBar);
    }),

    vscode.commands.registerCommand('harness.explainCode', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const text = editor.document.getText(editor.selection);
      if (!text) { vscode.window.showInformationMessage('Select code to explain.'); return; }
      chatPanel.show(context);
    }),

    vscode.commands.registerCommand('harness.fixDiagnostics', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const diags = vscode.languages.getDiagnostics(editor.document.uri);
      if (!diags.length) { vscode.window.showInformationMessage('No diagnostics to fix.'); return; }
      chatPanel.show(context);
    })
  );

  // Context menu
  context.subscriptions.push(
    vscode.languages.registerCodeActionsProvider('*', {
      provideCodeActions(doc, range, ctx) {
        if (ctx.diagnostics.length === 0) return [];
        const action = new vscode.CodeAction('Fix with Harness', vscode.CodeActionKind.QuickFix);
        action.command = { command: 'harness.fixDiagnostics', title: 'Fix with Harness' };
        return [action];
      }
    }, { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] })
  );
}

export function deactivate() {}
