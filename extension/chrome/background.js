const BRIDGE = "http://127.0.0.1:39276";
const BROWSER = "chrome";
const SYNC_MS = 750;
const COMMAND_MS = 350;

async function snapshotTabs() {
  const tabs = await chrome.tabs.query({});
  return {
    browser: BROWSER,
    tabs: tabs
      .filter((tab) => tab.id !== undefined && tab.windowId !== undefined && tab.title)
      .map((tab) => ({
        browser: BROWSER,
        windowId: tab.windowId,
        tabId: tab.id,
        title: tab.title || "",
        active: Boolean(tab.active),
      })),
  };
}

async function postTabs() {
  try {
    const body = await snapshotTabs();
    await fetch(`${BRIDGE}/tabs`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch (_) {
    // The native app is optional; failures are expected when it is closed.
  } finally {
    pollCommands();
  }
}

async function pollCommands() {
  try {
    const response = await fetch(`${BRIDGE}/commands`);
    const payload = await response.json();
    for (const command of payload.commands || []) {
      if (command.type !== "activate_tab" || command.browser !== BROWSER) {
        continue;
      }
      await chrome.windows.update(command.windowId, { focused: true });
      await chrome.tabs.update(command.tabId, { active: true });
    }
  } catch (_) {
    // The native app is optional; failures are expected when it is closed.
  }
}

chrome.tabs.onCreated.addListener(postTabs);
chrome.tabs.onUpdated.addListener(postTabs);
chrome.tabs.onRemoved.addListener(postTabs);
chrome.tabs.onActivated.addListener(postTabs);
chrome.windows.onFocusChanged.addListener(postTabs);

function setupAlarms() {
  try {
    chrome.alarms.create("mega-win-alt-tab-sync", { periodInMinutes: 1 });
    chrome.alarms.create("mega-win-alt-tab-commands", { periodInMinutes: 1 });
  } catch (_) {
    // Older or restricted extension contexts may not expose alarms.
  }
}

chrome.runtime.onInstalled.addListener(() => {
  setupAlarms();
  postTabs();
});

chrome.runtime.onStartup.addListener(() => {
  setupAlarms();
  postTabs();
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === "mega-win-alt-tab-sync") {
    postTabs();
  }
  if (alarm.name === "mega-win-alt-tab-commands") {
    pollCommands();
  }
});

setInterval(postTabs, SYNC_MS);
setInterval(pollCommands, COMMAND_MS);
setupAlarms();
postTabs();
