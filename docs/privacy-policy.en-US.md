# ShareOneList Privacy Policy

**Effective date: September 22, 2026 | Applies to: v2.2.0 and later**

This Privacy Policy explains how ShareOneList (the "app") handles your data. Please read it carefully before using the app. Installing or using the app means you have understood and agreed to this policy.

- Developer: ShareOneList Contributors
- App identifier: `com.shareonelist.app`
- Project home: https://github.com/ituff/ShareOneList

## 1. Core Data-Handling Principles

ShareOneList is designed **local-first**:

- The app has **no developer-owned servers** and contains no telemetry, usage analytics, crash reporting, or advertising SDKs.
- The developer **cannot** access your Microsoft account, your cloud files, your chat history, or any of your configuration data.
- Your data is stored only **on your own device**, or in the Microsoft cloud **of your own account**, accessed with your credentials.
- We do not sell, rent, or otherwise provide your personal data to any third party.

## 2. Data Stored on Your Device

All of the following data is kept in the app's local data directory and the OS credential store, and is used solely to operate the app:

| Category | Contents | Storage location |
|---|---|---|
| Account info | Account ID, display name, drive ID, cloud environment (Global / 21Vianet), your custom alias and icon | Local config file `config.json` |
| Sign-in credentials | Microsoft OAuth access and refresh tokens | OS credential store (Windows Credential Manager / macOS Keychain); never written in plain text to files |
| App settings | Theme, language, window state, download folder, update channel, backup folder, etc. | Local config file `config.json` |
| AI provider config | Provider name, API base URL, model name | Local config file `llm.json` |
| AI API keys | The API keys you enter for model providers | OS credential store; **never** written to JSON config files |
| Chat history & AI memory | Conversations with the AI assistant, and long-term memories extracted from them (e.g. preferences, frequently used locations) | Local SQLite database `chat_history.db` |
| Directory index (Drive Catalog) | Metadata index (names, paths, IDs) of cloud folders you have browsed, used to speed up search | Local SQLite database `catalog.db` |
| Downloaded files | Contents of files you choose to download | A location you choose on your device |

All of the above data is isolated per signed-in account and is never shared across accounts.

## 3. When Data Leaves Your Device

The app communicates over the network in only the following three cases. There is **no other outbound data transfer** of any kind.

### 3.1 Microsoft Sign-in and Graph API

- **Sign-in**: The app opens the Microsoft sign-in page in your browser (Global: `login.microsoftonline.com`; 21Vianet: `login.partner.microsoftonline.cn`). Your credentials are handled by Microsoft directly; the app never sees your password.
- **File operations**: Browsing, searching, uploading, downloading, and sharing are performed through the Microsoft Graph API (Global: `graph.microsoft.com`; 21Vianet: `microsoftgraph.chinacloudapi.cn`) using your OAuth tokens. This data is processed by Microsoft under the [Microsoft Privacy Statement](https://privacy.microsoft.com/privacystatement).

### 3.2 AI Assistant (your own configured model provider)

When you use the "Ask AI" feature, the following is sent to the **LLM provider endpoint you configure yourself** in the app (e.g. OpenAI, Azure OpenAI, Anthropic, or another compatible service), authenticated with your own API key:

- Your questions and the conversation context;
- When the AI needs to cite a cloud file in its answer, a **text excerpt** of that file, extracted locally by the app;
- The AI memory feature sends a window of recent conversation to the model to extract long-term memories; the extracted memories are saved only to your local database.

Content you send to a provider is governed by that provider's own privacy policy — please review it and choose providers carefully. If you do not want any file content to leave your device, avoid having the AI cite files, or leave the AI provider unconfigured. The app's built-in system prompt is held by the app itself; system messages you type are not sent to the provider.

### 3.3 Update Check

- The app contacts the GitHub Releases API (`api.github.com`) to check for new versions;
- Installer downloads may be routed through third-party acceleration mirrors (`ghfast.top`, `gh-proxy.com`, `github.moeyy.xyz`) to improve download speeds in mainland China;
- This traffic is an ordinary HTTP request that **contains no** account, file, or identity data. You can also check for updates manually in Settings.

## 4. WebDAV Mounting

The WebDAV mount feature runs a gateway on your machine so OneDrive/SharePoint folders can be mapped in File Explorer or Finder. The gateway **listens only on the loopback address `127.0.0.1`** and validates the Host header of every request; it is never exposed to your local network or the internet.

## 5. Backup and Export

You can export your settings and account info to a local file, or have them backed up automatically to a folder you choose in your own OneDrive (uploaded through Microsoft Graph with your signed-in account). By default backups contain **no credentials**. You may opt in to including sensitive data — sign-in refresh tokens, AI provider API keys, chat history, WebDAV gateway password — and whenever you do, those sections are encrypted with AES-256-GCM using a key derived from your own backup password; the password itself is never uploaded and is required to decrypt on restore. Backup files are yours to keep — do not share them with parties you do not trust.

## 6. Data Retention and Deletion

You have full control over all of this data:

- **Sign out**: Clears the OAuth tokens and local session for that account.
- **In-app deletion**: You can delete chat history, AI memories, and the directory index at any time (the app also deletes and rebuilds these SQLite databases automatically if they become corrupted).
- **Full removal**: After uninstalling, delete the app data directory to remove all local data:
  - Windows: `%APPDATA%\com.shareonelist.app`
  - macOS: `~/Library/Application Support/com.shareonelist.app`
- Data already sent to an LLM provider is governed by that provider's policy; files stored in your Microsoft cloud remain under the control of your Microsoft account.

## 7. Children's Privacy

The app is not directed to children under 13 (under 16 in the EU), and we do not knowingly collect personal information from children.

## 8. Your Rights

Under applicable law (including the EU GDPR, the US CCPA, and China's PIPL), you have rights to access, correct, delete, port, and restrict processing of your personal data. Because all of the app's data lives on your own device and in your own Microsoft account, you can exercise these rights directly (see Section 6). For anything else, contact us through the channel in Section 10.

## 9. International Data Transfers

- Overseas LLM providers you configure may process and store conversation data on servers outside your country, as described in their own policies.
- Data processed by the Microsoft cloud (Global and 21Vianet environments) is subject to Microsoft's respective regional privacy commitments.

## 10. Contact Us

For any questions, feedback, or data-protection requests regarding this Privacy Policy:

- GitHub Issues: https://github.com/ituff/ShareOneList/issues

## 11. Changes to This Policy

We may revise this policy as the app is updated. Revised policies are published on this page with an updated effective date. If a change materially reduces your rights, we will provide a more prominent notice.
