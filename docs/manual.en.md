# rs-ft8n PWA User Manual

**[Japanese version](manual.md)** | **[Open App](https://jl1nie.github.io/rs-ft8n/)**

rs-ft8n is a browser-based FT8 QSO application -- decode, transmit, CAT control, and log management in a single PWA. No installation required -- works on Chrome, Edge, and Safari (WebKit).

---

## Screen Layout

```
+--------------------------------------+
| rs-ft8n [Scout][Snipe]    12s (213ms)| <- Header (mode, secs left, decode time)
|--------------------------------------|
| ::::::::: Waterfall ::::::::::::::::: | <- Waterfall (tap to set DF)
| ::::::::|::::::::::::::::::::::::::::: |    Red dashed line = current DF
|--------------------------------------|
|  1234 +0.3  -12  CQ 3Y0Z JD34       | <- DF  DT  SNR  Message
|   800 +0.1   -8  JA1ABC 3Y0Z PM95   |
|--------------------------------------|
| [CQ]  [Halt] Auto    12 decoded     | <- TX actions + status
+--------------------------------------+
```

---

## Initial Setup

1. Open the [app](https://jl1nie.github.io/rs-ft8n/)
2. The settings panel (gear icon in the top right) opens automatically
3. Enter the following:
   - **My Callsign** -- your callsign (e.g., `W1AW`)
   - **My Grid** -- your grid locator (e.g., `FN31`)
   - **Audio Input** -- select your USB audio interface (receive side)
   - **Audio Output** -- select TX audio output device (transmit side)
4. Tap **Start Audio** to begin live decoding

> For CAT control, select your CAT Protocol and tap **Connect CAT**. Requires a Web Serial API compatible browser (Chrome / Edge).

---

## Two Modes

### Scout Mode -- Casual CQ Operation

A chat-style UI for relaxed QSOs. Ideal for portable operation and smartphone use.

**Basic operations:**

| Action | Effect |
|--------|--------|
| **CQ button** | Queue CQ for the next period |
| **Tap waterfall** | Set TX frequency (DF). Shown as a red dashed vertical line |
| **Tap an RX message** | Call the sender (call2) of that message (auto-starts QSO) |
| **Auto checkbox** | ON: automatic responses. OFF: manually select TX messages |
| **Halt** | Immediately stop transmission |

**Message display (unified format):**

```
 DF(Hz)  DT(s)  SNR  Message
  1234   +0.3   -12  CQ 3Y0Z JD34
   800   +0.1    -8  JA1ABC 3Y0Z PM95
```

- Blue border = received message (RX)
- Grey border = transmitted message (TX)
- **Yellow bold** = your callsign
- **Green bold** = QSO partner's callsign

**QSO flow (Auto mode):**

1. Tap the waterfall to set DF
2. Tap CQ to queue for next period, or tap an RX message to call that station
3. Response received -- automatic report exchange
4. RR73 / 73 -- QSO complete (auto-saved to log)

**Scout AP decoding:**

During an active QSO (when the DX station is known), A Priori (AP) decoding is automatically enabled to improve reception of weak signals from the target. If decode time exceeds the FT8 budget (2.4 seconds), AP and subtract are automatically paused (see [Adaptive Budget](#adaptive-budget-scout-mode)).

---

### Snipe Mode -- DX Hunting

A dedicated mode for hunting target stations. The waterfall is larger.

#### Watch Phase (Full-Band Receive)

Find the target and choose a calling frequency.

| Action | Effect |
|--------|--------|
| **DX Call (AP)** in settings | Set target station (enables AP decoding) |
| **Tap waterfall** | Set DF (red dashed line). In Call phase this becomes the 500 Hz window center |
| **Tap an RX message** | Set call1 as target. A secondary "Call call2" button also appears in TX actions |

**Watch display:**

- **Top**: Target station's latest message (call / frequency / SNR)
- **Callers**: List of other stations calling the target
- **Message list**: DF / DT / SNR / Message in unified format
- **QSO progress dots**: filled-circle empty-circle empty-circle empty-circle -> all filled

#### Call Phase (Keep Calling)

Once you've set the DF in Watch, switch to **Call**.

- 500 Hz BPF window shown as a cyan band on the waterfall
- Only messages involving you and the target are displayed (noise reduction)
- Automatically starts calling the target
- QSO failure (retry limit reached) -- auto-reverts to Watch
- You can manually switch back to Watch to change DF

**Switching Watch / Call:**

Use the `[Watch] [Call]` tabs at the top of the Snipe view. The message list is preserved when switching.

---

## Waterfall

Real-time spectrogram covering 200-2800 Hz.

| Element | Description |
|---------|-------------|
| **Top labels** | Frequency axis (200, 500, 1000, 1500, 2000, 2500 Hz) |
| **Red dashed line (vertical)** | DF (TX frequency) -- shown in all modes on tap |
| **Cyan band** | Snipe Call phase 500 Hz BPF window |
| **Red dashed line (horizontal)** | Period boundary (15-second intervals) |
| **Yellow text** | Decoded messages |
| **Tap / click** | Set DF + status bar shows frequency |
| **WAV drag & drop** | Offline analysis (auto-stops live audio) |

---

## Header Display

```
rs-ft8n [Scout][Snipe]   12s (213ms)
```

- **Mode tabs**: Scout / Snipe toggle
- **Seconds remaining**: Integer seconds until next period boundary
- **Decode time**: Time taken for the last decode cycle (ms)

---

## TX Actions (Bottom Bar)

Buttons change dynamically based on QSO state:

| QSO State | Auto ON | Auto OFF |
|-----------|---------|----------|
| **IDLE** | `[CQ]` button | `[CQ]` button |
| **Active QSO** | Single next-TX message (tap to send) | All TX options as buttons |

- **Snipe**: A secondary `[Call xxx]` button appears when call2 differs from the target
- **Halt**: Immediately stops transmission
- CQ is queued on tap and transmitted at the next period start

---

## QSO State Machine

QSO management uses a 4-state progression:

```
IDLE -> CALLING -> REPORT -> FINAL -> IDLE (complete)
```

| State | Meaning | TX content |
|-------|---------|------------|
| **IDLE** | Standby | -- |
| **CALLING** | Calling | `DX MYCALL GRID` or `CQ MYCALL GRID` |
| **REPORT** | Report exchange | `DX MYCALL R+00` |
| **FINAL** | Awaiting confirmation | `DX MYCALL RR73` or `73` |

- **Auto ON**: Fully automatic state transitions. Retries on no response (up to 15 times).
- **Auto OFF**: TX message selector buttons appear for manual selection.
- When the retry limit is reached, the QSO is logged as incomplete.

---

## Adaptive Budget (Scout Mode)

In Scout mode, when decode time exceeds the FT8 budget (2.4 seconds), heavy features are automatically paused in order:

| Priority | Feature | Load |
|----------|---------|------|
| 1 (OFF first) | Multi-pass subtract | 3-pass SIC (heaviest) |
| 2 (OFF next) | A Priori (AP) | Sniper + EQ single pass |

The status bar shows `[-sub]` or `[-sub,AP]` when features are paused.

Recovery at 60% budget headroom, in reverse order:
1. AP re-enabled first
2. Subtract re-enabled when budget allows

Snipe mode always runs both features (narrow band = fast enough).

---

## Callsign Hash Resolution

Some FT8 message types (Type 4 non-standard calls, DXpedition messages, etc.) transmit callsigns as 10/12/22-bit hashes. Normally displayed as `<...>`, rs-ft8n automatically builds a **hash table from decoded callsigns** and resolves hashes to actual calls like `<JA1ABC>` in subsequent messages.

The table accumulates during the WASM session (max 1000 entries, LRU eviction) and clears on page reload.

---

## Settings Panel

Open/close with the gear icon.

| Item | Description |
|------|-------------|
| **My Callsign** | Your callsign |
| **My Grid** | Grid locator (4 characters) |
| **Audio Input** | Receive audio device |
| **Audio Output** | TX audio output device |
| **DX Call (AP)** | Target station call (AP decoding + Snipe target) |
| **CAT Protocol** | Yaesu (FTDX10) / Icom (CI-V) |
| **Connect CAT** | Connect to rig via Web Serial (PTT control) |
| **Start / Stop Audio** | Start or stop live decoding |
| **Reset QSO** | Abort QSO (saved as incomplete to log) |
| **Open WAV File** | Select a WAV file for offline analysis |
| **Multi-pass subtract** | Successive interference cancellation (3-pass SIC). Default ON |
| **A Priori (AP) decode** | AP decoding. Default ON. Scout auto-pauses when over budget |

---

## Log Management

### Auto-Save

- **QSO complete**: Automatically saved to localStorage (state = IDLE)
- **QSO aborted**: Saved as incomplete on Reset or retry timeout (state = CALLING/REPORT/FINAL)
- **All RX messages**: All decoded messages accumulated in RX log (max 10,000 entries)

### ZIP Export

Tap **Export ZIP (ADIF + RX)** in the settings panel to download a ZIP containing 3 files:

| File | Contents |
|------|----------|
| `qso_complete_YYYYMMDD.adi` | Completed QSOs only (for LoTW / Club Log upload) |
| `qso_all_YYYYMMDD.adi` | All QSOs (incomplete entries include `<COMMENT>incomplete:STATE</COMMENT>`) |
| `rx_YYYYMMDD.csv` | All received decode log (UTC, Freq, SNR, Message) |

### Clearing Logs

**Clear All Logs** button deletes both QSO and RX logs (confirmation dialog shown).

---

## Offline Analysis with WAV Files

Drag & drop a WAV file onto the waterfall, or use **Open WAV File** in the settings panel.

- **Requirements**: 12 kHz / 16-bit / mono WAV
- Auto-stops live audio if active
- Waterfall + decode results displayed immediately

---

## CAT Control

Uses the Web Serial API to control rig PTT from the browser.

**Supported protocols:**

| Protocol | Example rigs | Baud rate |
|----------|-------------|-----------|
| **Yaesu** | FTDX10, FT-991A, etc. | 38400 |
| **Icom CI-V** | IC-7300, IC-705, etc. | 19200 |

**Connection steps:**

1. Connect your rig to the PC via USB cable
2. Select CAT Protocol in the settings panel
3. **Connect CAT** -- browser serial port selection dialog opens
4. Select the port -- connected
5. PTT is automatically controlled during TX

> Web Serial API is only available in Chrome / Edge. Safari / Firefox are not supported.

---

## Troubleshooting

| Symptom | Solution |
|---------|----------|
| "Select audio device" shown | Select Audio Input in the settings panel |
| 0 decodes | Check antenna, audio level, and frequency (e.g., 14.074 MHz) |
| WAV drop error | Verify 12 kHz / 16-bit / mono format. 48 kHz WAV is not supported |
| CAT won't connect | Use Chrome / Edge. Restart browser and retry |
| Waterfall is black | Verify the correct Audio Input device. Press Start Audio again |
| QSO not progressing | Check that Auto is ON. Verify DX Call is correct |
| `[-sub]` `[-sub,AP]` shown | Decode is too heavy; features auto-paused. Will recover when budget allows |
| `<...>` not resolved | That station hasn't been decoded in this session. Hash table clears on page reload |

---

## System Requirements

| Item | Requirement |
|------|-------------|
| **Browser** | Chrome 90+, Edge 90+, Safari 15+ |
| **Web Serial** | Chrome / Edge only (required for CAT control) |
| **Audio** | getUserMedia support (HTTPS or localhost) |
| **WASM** | WebAssembly support (all modern browsers) |
| **Display** | Mobile-friendly (responsive layout) |
