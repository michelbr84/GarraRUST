import os
import re

import sys

target_dir = r"g:\Projetos\GarraRUST\crates\garraia-gateway\assets"
app_js_path = os.path.join(target_dir, "app.js")

with open(app_js_path, "r", encoding="utf-8") as f:
    content = f.read()

def extract_section(start_marker, end_marker):
    start_idx = content.find(start_marker)
    if start_idx == -1: return ""
    end_idx = content.find(end_marker, start_idx + len(start_marker)) if end_marker else len(content)
    return content[start_idx:end_idx].strip()

# Defining the sections
sec1 = extract_section("// 1. Core State & Event Bus", "// ===============================")
sec2 = extract_section("// 2. DOM Elements", "// ===============================")
sec3 = extract_section("// 3. I18N & Themes", "// ===============================")
sec4 = extract_section("// 4. View Router", "// ===============================")
sec5 = extract_section("// 5. Memory & Logs Logic", "// ===============================")
sec6 = extract_section("// 6. Chat & WebSocket Logic", "// ===============================")
sec7 = extract_section("// 7. API Integrations", "")

os.makedirs(os.path.join(target_dir, "views"), exist_ok=True)

# 1. eventBus.js
event_bus_code = "export const EventBus = {\n" + sec1.split("const EventBus = {")[1].split("const GarraState =")[0].strip()
with open(os.path.join(target_dir, "eventBus.js"), "w", encoding="utf-8") as f:
    f.write(event_bus_code)

# 2. state.js
state_code = 'import { EventBus } from "./eventBus.js";\n\n' + "export const GarraState = " + sec1.split("const GarraState =")[1].strip()
with open(os.path.join(target_dir, "state.js"), "w", encoding="utf-8") as f:
    f.write(state_code)

# 3. dom.js
dom_code = "export const dom = {\n" + sec2.split("const dom = {")[1].strip()
with open(os.path.join(target_dir, "dom.js"), "w", encoding="utf-8") as f:
    f.write(dom_code)

# 4. i18n_theme.js (renamed)
i18n_code = """import { dom } from './dom.js';
""" + sec3.replace("function applyTheme", "export function applyTheme").replace("function initTheme", "export function initTheme")
with open(os.path.join(target_dir, "theme.js"), "w", encoding="utf-8") as f:
    f.write(i18n_code)

# 5. router.js
router_code = """import { GarraState } from './state.js';
import { EventBus } from './eventBus.js';
import { loadMemory } from './views/memoryView.js';
import { loadLogs } from './views/logsView.js';

""" + sec4.replace("function initRouter", "export function initRouter")
with open(os.path.join(target_dir, "router.js"), "w", encoding="utf-8") as f:
    f.write(router_code)

# 6. views/memoryView.js and logsView.js
# Extracted from sec5
# Simple regex to split memory and logs logic
memory_logic = sec5.split("// Logs Logic")[0]
logs_logic = "// Logs Logic\n" + sec5.split("// Logs Logic")[1]

memory_code = """import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { dom } from '../dom.js';

""" + memory_logic.replace("async function loadMemory", "export async function loadMemory")

with open(os.path.join(target_dir, "views", "memoryView.js"), "w", encoding="utf-8") as f:
    f.write(memory_code)

logs_code = """import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { dom } from '../dom.js';

""" + logs_logic.replace("async function loadLogs", "export async function loadLogs")

with open(os.path.join(target_dir, "views", "logsView.js"), "w", encoding="utf-8") as f:
    f.write(logs_code)

# 7. api.js
api_code = """import { GarraState } from './state.js';
import { EventBus } from './eventBus.js';
import { dom } from './dom.js';

""" + sec7.replace("async function fetchMCPs", "export async function fetchMCPs")\
    .replace("async function fetchProviders", "export async function fetchProviders")
with open(os.path.join(target_dir, "api.js"), "w", encoding="utf-8") as f:
    f.write(api_code)

# 8. views/chatView.js
chat_code = """import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { dom } from '../dom.js';

""" + sec6.replace("function connect", "export function connect")\
    .replace("function sendMessage", "export function sendMessage")\
    .replace("function initChat", "export function initChat")

with open(os.path.join(target_dir, "views", "chatView.js"), "w", encoding="utf-8") as f:
    f.write(chat_code)

# 9. Write bootstrap app.js
bootstrap_code = """import { initTheme } from './theme.js';
import { initRouter } from './router.js';
import { connect, initChat } from './views/chatView.js';
import { fetchProviders, fetchMCPs } from './api.js';
import './views/memoryView.js';
import './views/logsView.js';

document.addEventListener("DOMContentLoaded", () => {
    initTheme();
    initRouter();
    initChat();
    connect();
    fetchProviders();
    fetchMCPs();
});
"""
with open(app_js_path, "w", encoding="utf-8") as f:
    f.write(bootstrap_code)

print("Split complete!")
