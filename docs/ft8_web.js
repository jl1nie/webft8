/* @ts-self-types="./ft8_web.d.ts" */

export class DecodedMessage {
    static __wrap(ptr) {
        const obj = Object.create(DecodedMessage.prototype);
        obj.__wbg_ptr = ptr;
        DecodedMessageFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        DecodedMessageFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_decodedmessage_free(ptr, 0);
    }
    /**
     * @returns {string}
     */
    get message() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.decodedmessage_message(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    get dt_sec() {
        const ret = wasm.__wbg_get_decodedmessage_dt_sec(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get freq_hz() {
        const ret = wasm.__wbg_get_decodedmessage_freq_hz(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get hard_errors() {
        const ret = wasm.__wbg_get_decodedmessage_hard_errors(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get pass() {
        const ret = wasm.__wbg_get_decodedmessage_pass(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get snr_db() {
        const ret = wasm.__wbg_get_decodedmessage_snr_db(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {number} arg0
     */
    set dt_sec(arg0) {
        wasm.__wbg_set_decodedmessage_dt_sec(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set freq_hz(arg0) {
        wasm.__wbg_set_decodedmessage_freq_hz(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set hard_errors(arg0) {
        wasm.__wbg_set_decodedmessage_hard_errors(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set pass(arg0) {
        wasm.__wbg_set_decodedmessage_pass(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set snr_db(arg0) {
        wasm.__wbg_set_decodedmessage_snr_db(this.__wbg_ptr, arg0);
    }
}
if (Symbol.dispose) DecodedMessage.prototype[Symbol.dispose] = DecodedMessage.prototype.free;

/**
 * i16 variant of [`bootstrap_dt_f32`].
 * @param {Int16Array} samples
 * @param {number} sample_rate
 * @returns {number | undefined}
 */
export function bootstrap_dt(samples, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.bootstrap_dt(ptr0, len0, sample_rate);
    return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
}

/**
 * Cold-start DT estimate from `coarse_sync` candidates.
 *
 * Returns the DT median of the top-5 highest-score coarse-sync candidates
 * (mfsk-core 0.6.6 `bootstrap_dt_median`), which lands within ±100 ms of
 * the confirmed-decode DT median on reference recordings — useful for
 * seeding the JS-side period manager when the device clock is skewed >2 s
 * from UTC and no confirmed decode can be obtained yet.
 *
 * Returns `None` (→ `undefined` in JS) when no candidates are found.
 * @param {Float32Array} samples
 * @param {number} sample_rate
 * @returns {number | undefined}
 */
export function bootstrap_dt_f32(samples, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.bootstrap_dt_f32(ptr0, len0, sample_rate);
    return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
}

/**
 * Decode an FST4 slot (wide-band scan). `submode` 0..=4 picks the T/R
 * period (15/30/60/120/300 s); `profile` (0=Fast/1=Normal/2=Deep) maps
 * to `DecodeStrictness`. Non-12 kHz input is auto-resampled.
 * @param {Int16Array} samples
 * @param {number} submode
 * @param {number} profile
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_fst4_wav(samples, submode, profile, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_fst4_wav(ptr0, len0, submode, profile, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_fst4_wav`].
 * @param {Float32Array} samples
 * @param {number} submode
 * @param {number} profile
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_fst4_wav_f32(samples, submode, profile, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_fst4_wav_f32(ptr0, len0, submode, profile, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Streaming sibling of [`decode_fst4_wav`]: same wide-band scan, plus
 * `on_result(msg)` once per accepted candidate as it's found — most
 * valuable here of all four protocols, since FST4 slots run 15-300 s
 * (vs FT8's 15 s), so the old "nothing until the whole slot is decoded"
 * wait was the longest.
 * @param {Int16Array} samples
 * @param {number} submode
 * @param {number} profile
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_fst4_wav_streaming(samples, submode, profile, sample_rate, on_result) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_fst4_wav_streaming(ptr0, len0, submode, profile, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_fst4_wav_streaming`].
 * @param {Float32Array} samples
 * @param {number} submode
 * @param {number} profile
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_fst4_wav_streaming_f32(samples, submode, profile, sample_rate, on_result) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_fst4_wav_streaming_f32(ptr0, len0, submode, profile, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * FT4 sniper-mode decode at a target frequency with optional AP hints.
 * @param {Int16Array} samples
 * @param {number} target_freq
 * @param {string} callsign
 * @param {string} mycall
 * @param {boolean} eq_on
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_sniper(samples, target_freq, callsign, mycall, eq_on, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(callsign, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(mycall, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_sniper(ptr0, len0, target_freq, ptr1, len1, ptr2, len2, eq_on, sample_rate);
    var v4 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v4;
}

/**
 * f32 variant of [`decode_ft4_sniper`].
 * @param {Float32Array} samples
 * @param {number} target_freq
 * @param {string} callsign
 * @param {string} mycall
 * @param {boolean} eq_on
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_sniper_f32(samples, target_freq, callsign, mycall, eq_on, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(callsign, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(mycall, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_sniper_f32(ptr0, len0, target_freq, ptr1, len1, ptr2, len2, eq_on, sample_rate);
    var v4 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v4;
}

/**
 * Decode a 7.5-second FT4 slot (wide-band scan). Non-12 kHz input is
 * resampled automatically.
 * @param {Int16Array} samples
 * @param {number} strictness
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_wav(samples, strictness, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_wav(ptr0, len0, strictness, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_ft4_wav`].
 * @param {Float32Array} samples
 * @param {number} strictness
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_wav_f32(samples, strictness, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_wav_f32(ptr0, len0, strictness, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * FT4 multi-pass subtract decode (SIC) for crowded slots. `profile`
 * (0=Fast/1=Normal/2=Deep) picks both strictness and SIC round count —
 * 2 rounds for Fast, 3 (full) for Normal/Deep (see `wants_light_sic`).
 * @param {Int16Array} samples
 * @param {number} profile
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_wav_subtract(samples, profile, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_wav_subtract(ptr0, len0, profile, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_ft4_wav_subtract`].
 * @param {Float32Array} samples
 * @param {number} profile
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_wav_subtract_f32(samples, profile, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_wav_subtract_f32(ptr0, len0, profile, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Streaming sibling of [`decode_ft4_wav_subtract`]: same SIC decode, plus
 * `on_result(msg)` once per accepted candidate as it's found (mfsk-core
 * 0.9 `.on_result()`). `decode_ft4_wav_subtract` itself is untouched.
 * @param {Int16Array} samples
 * @param {number} profile
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_wav_subtract_streaming(samples, profile, sample_rate, on_result) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_wav_subtract_streaming(ptr0, len0, profile, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_ft4_wav_subtract_streaming`].
 * @param {Float32Array} samples
 * @param {number} profile
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_ft4_wav_subtract_streaming_f32(samples, profile, sample_rate, on_result) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_ft4_wav_subtract_streaming_f32(ptr0, len0, profile, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Phase 1 decode (i16): fast single-pass decode.
 *
 * Caches the resampled audio and FFT for a subsequent `decode_phase2` call.
 * @param {Int16Array} samples
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_phase1(samples, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_phase1(ptr0, len0, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Phase 1 decode (f32): fast single-pass decode for live AudioWorklet path.
 *
 * Caches the resampled audio and FFT for a subsequent `decode_phase2_f32` call.
 * @param {Float32Array} samples
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_phase1_f32(samples, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_phase1_f32(ptr0, len0, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Phase 1 decode (i16), streaming: identical to `decode_phase1`, but calls
 * `on_result(msg)` once per accepted candidate as Phase 1 finds it, in
 * addition to returning the full batch at the end.
 * @param {Int16Array} samples
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_phase1_streaming(samples, sample_rate, on_result) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_phase1_streaming(ptr0, len0, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Phase 1 decode (f32), streaming: `decode_phase1_f32` + per-candidate
 * `on_result(msg)` delivery, for the live AudioWorklet path.
 * @param {Float32Array} samples
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_phase1_streaming_f32(samples, sample_rate, on_result) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_phase1_streaming_f32(ptr0, len0, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Phase 2 decode (i16): SIC using cached Phase 1 state, strength picked by
 * the GUI decode-profile level (see `wants_normal_sic`). `profile == 0`
 * (Fast) is not specially handled here — callers wanting Fast's "Phase 1
 * alone, no SIC at all" semantics skip calling this at all (see
 * `wants_normal_sic`'s doc comment); calling it with `profile == 0`
 * anyway just runs the full `.sic_early()` strategy, same as Deep.
 *
 * Panics if `decode_phase1` was not called first. Prior to mfsk-core
 * commit fe286cc / issue #191, this call went through a separate,
 * unfixed flat-3-pass engine (`decode_frame_subtract_with_known`) that
 * never received the staged-checkpoint SIC recall improvements
 * `decode_wav_subtract` got — `known`/`fft_cache` are now honoured
 * directly by `.sic_early()` (renamed from `.staged()` in mfsk-core
 * #218), so this is the same engine as every other subtract path.
 * @param {number} profile
 * @returns {DecodedMessage[]}
 */
export function decode_phase2(profile) {
    const ret = wasm.decode_phase2(profile);
    var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v1;
}

/**
 * Phase 2 decode (f32): SIC using cached Phase 1 state, strength picked by
 * the GUI decode-profile level (see `wants_normal_sic`). `profile == 0`
 * (Fast) is not specially handled here — callers wanting Fast's "Phase 1
 * alone, no SIC at all" semantics skip calling this at all (see
 * `wants_normal_sic`'s doc comment); calling it with `profile == 0`
 * anyway just runs the full `.sic_early()` strategy, same as Deep.
 *
 * Panics if `decode_phase1_f32` was not called first. See `decode_phase2`
 * for why this now shares the same staged-checkpoint SIC engine as
 * `decode_wav_subtract_f32`.
 * @param {number} profile
 * @returns {DecodedMessage[]}
 */
export function decode_phase2_f32(profile) {
    const ret = wasm.decode_phase2_f32(profile);
    var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v1;
}

/**
 * Phase 2 decode (i16), streaming: identical to `decode_phase2`, but calls
 * `on_result(msg)` once per accepted SIC candidate as it's found.
 *
 * Panics if `decode_phase1`/`decode_phase1_streaming` was not called first.
 * @param {number} profile
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_phase2_streaming(profile, on_result) {
    const ret = wasm.decode_phase2_streaming(profile, on_result);
    var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v1;
}

/**
 * Phase 2 decode (f32), streaming: `decode_phase2_f32` + per-candidate
 * `on_result(msg)` delivery.
 *
 * Panics if `decode_phase1_f32`/`decode_phase1_streaming_f32` was not
 * called first.
 * @param {number} profile
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_phase2_streaming_f32(profile, on_result) {
    const ret = wasm.decode_phase2_streaming_f32(profile, on_result);
    var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v1;
}

/**
 * Plain Q65 BP decode. i16 audio variant.
 * @param {Int16Array} samples
 * @param {number} submode
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav(samples, submode, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav(ptr0, len0, submode, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Plain Q65 BP decode (basic AWGN strategy). f32 audio.
 * @param {Float32Array} samples
 * @param {number} submode
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_f32(samples, submode, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_f32(ptr0, len0, submode, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 → i16 wrapper for the fast-fading variant. `b90_ts` and
 * `model` semantics identical to [`decode_q65_wav_fading_f32`].
 * @param {Int16Array} samples
 * @param {number} submode
 * @param {number} b90_ts
 * @param {number} model
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_fading(samples, submode, b90_ts, model, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_fading(ptr0, len0, submode, b90_ts, model, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Q65 fast-fading metric decode (high-Doppler EME).
 *
 * `b90_ts` is the spread-bandwidth × symbol-period dimensionless
 * product. Calibrated test values: 3 (light spread), 8 (moderate),
 * 15 (heavy / 10+ GHz EME). `model`: 0 = Gaussian, 1 = Lorentzian.
 * @param {Float32Array} samples
 * @param {number} submode
 * @param {number} b90_ts
 * @param {number} model
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_fading_f32(samples, submode, b90_ts, model, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_fading_f32(ptr0, len0, submode, b90_ts, model, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Streaming sibling of [`decode_q65_wav_fading`]: same fast-fading metric
 * decode, plus `on_result(msg)` once per accepted candidate.
 * @param {Int16Array} samples
 * @param {number} submode
 * @param {number} b90_ts
 * @param {number} model
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_fading_streaming(samples, submode, b90_ts, model, sample_rate, on_result) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_fading_streaming(ptr0, len0, submode, b90_ts, model, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_q65_wav_fading_streaming`].
 * @param {Float32Array} samples
 * @param {number} submode
 * @param {number} b90_ts
 * @param {number} model
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_fading_streaming_f32(samples, submode, b90_ts, model, sample_rate, on_result) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_fading_streaming_f32(ptr0, len0, submode, b90_ts, model, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Streaming sibling of [`decode_q65_wav`]: same basic BP scan, plus
 * `on_result(msg)` once per accepted candidate as it's found.
 * @param {Int16Array} samples
 * @param {number} submode
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_streaming(samples, submode, sample_rate, on_result) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_streaming(ptr0, len0, submode, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_q65_wav_streaming`].
 * @param {Float32Array} samples
 * @param {number} submode
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_q65_wav_streaming_f32(samples, submode, sample_rate, on_result) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_q65_wav_streaming_f32(ptr0, len0, submode, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 *   mycall + dxcall + RRR/RR73/73 → 77-bit lock (passes 9-11)
 *   CQ + dxcall + grid → up to 76-bit lock (passes 7/8)
 *   mycall + dxcall → 61-bit lock (pass 8)
 *   dxcall only → 33-bit lock (pass 6)
 *   grid only → 15-bit lock (pass 6 fallback)
 *
 * Pass `mycall = ""` for Watch phase (CQ-style hint + grid).
 * Pass `mycall = <own_call>` for Call phase (QSO hint, grid ignored).
 * @param {Int16Array} samples
 * @param {number} target_freq
 * @param {string} callsign
 * @param {string} grid
 * @param {string} mycall
 * @param {boolean} eq_on
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_sniper(samples, target_freq, callsign, grid, mycall, eq_on, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(callsign, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(grid, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ptr3 = passStringToWasm0(mycall, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len3 = WASM_VECTOR_LEN;
    const ret = wasm.decode_sniper(ptr0, len0, target_freq, ptr1, len1, ptr2, len2, ptr3, len3, eq_on, sample_rate);
    var v5 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v5;
}

/**
 * f32 variant of `decode_sniper`. See `decode_sniper` for parameters.
 * @param {Float32Array} samples
 * @param {number} target_freq
 * @param {string} callsign
 * @param {string} grid
 * @param {string} mycall
 * @param {boolean} eq_on
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_sniper_f32(samples, target_freq, callsign, grid, mycall, eq_on, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(callsign, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(grid, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ptr3 = passStringToWasm0(mycall, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len3 = WASM_VECTOR_LEN;
    const ret = wasm.decode_sniper_f32(ptr0, len0, target_freq, ptr1, len1, ptr2, len2, ptr3, len3, eq_on, sample_rate);
    var v5 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v5;
}

/**
 * Decode a 15-second FT8 audio frame (wide-band scan).
 *
 * `sample_rate` — input PCM sample rate in Hz (e.g. 12000, 44100, 48000).
 * Non-12 000 Hz input is automatically resampled before decoding.
 * @param {Int16Array} samples
 * @param {number} strictness
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_wav(samples, strictness, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wav(ptr0, len0, strictness, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of `decode_wav`. See `decode_wav` for parameters.
 * @param {Float32Array} samples
 * @param {number} strictness
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_wav_f32(samples, strictness, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wav_f32(ptr0, len0, strictness, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Decode with iterative signal subtraction.
 *
 * `sample_rate` — input PCM sample rate in Hz. Non-12 000 Hz input is
 * automatically resampled before decoding.
 * @param {Int16Array} samples
 * @param {number} strictness
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_wav_subtract(samples, strictness, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wav_subtract(ptr0, len0, strictness, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of `decode_wav_subtract`. See `decode_wav_subtract` for parameters.
 * @param {Float32Array} samples
 * @param {number} strictness
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_wav_subtract_f32(samples, strictness, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wav_subtract_f32(ptr0, len0, strictness, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Decode a 120-s WSPR slot. Non-12 kHz input is auto-resampled. Runs
 * coarse (freq, time) search with the default time tolerance and
 * 1400-1600 Hz freq sweep, then Fano-decodes every candidate above
 * the sync-score threshold.
 * @param {Int16Array} samples
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_wspr_wav(samples, sample_rate) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wspr_wav(ptr0, len0, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_wspr_wav`].
 * @param {Float32Array} samples
 * @param {number} sample_rate
 * @returns {DecodedMessage[]}
 */
export function decode_wspr_wav_f32(samples, sample_rate) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wspr_wav_f32(ptr0, len0, sample_rate);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Streaming sibling of [`decode_wspr_wav`]: same 120-s scan (mfsk-core's
 * `decode_scan_streaming`, matching `decode_scan_default`'s params — see
 * that function), plus `on_result(msg)` once per accepted candidate.
 * WSPR's own delivery contract is the "parallel" one (both coarse passes
 * run under rayon) — dedup against `known` doesn't apply here (WSPR has
 * no cross-phase pipeline in this build), so unlike FT8/FT4/FST4's
 * `.known()` gap this path was never at risk of a post-hoc retract.
 * @param {Int16Array} samples
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_wspr_wav_streaming(samples, sample_rate, on_result) {
    const ptr0 = passArray16ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wspr_wav_streaming(ptr0, len0, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * f32 variant of [`decode_wspr_wav_streaming`].
 * @param {Float32Array} samples
 * @param {number} sample_rate
 * @param {Function} on_result
 * @returns {DecodedMessage[]}
 */
export function decode_wspr_wav_streaming_f32(samples, sample_rate, on_result) {
    const ptr0 = passArrayF32ToWasm0(samples, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.decode_wspr_wav_streaming_f32(ptr0, len0, sample_rate, on_result);
    var v2 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Encode a free-text FT8 message (Type 0, n3=0) as audio samples.
 *
 * `text` — up to 13 characters from the FT8 free-text alphabet.
 * @param {string} text
 * @param {number} freq_hz
 * @returns {Float32Array}
 */
export function encode_free_text(text, freq_hz) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.encode_free_text(ptr0, len0, freq_hz);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * Encode a standard FST4 message (CALL1 CALL2 GRID/REPORT) at the
 * requested sub-mode + audio centre frequency. `submode` 0..=4 picks
 * the T/R period (15/30/60/120/300 s), which only changes the GFSK
 * pulse-shaping constant — the 77-bit message packing and tone
 * sequence are sub-mode independent (shared with FT4/FT8). Returns
 * 12 kHz f32 PCM at amplitude 1.0.
 * @param {string} call1
 * @param {string} call2
 * @param {string} report
 * @param {number} freq_hz
 * @param {number} submode
 * @returns {Float32Array}
 */
export function encode_fst4(call1, call2, report, freq_hz, submode) {
    const ptr0 = passStringToWasm0(call1, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(call2, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(report, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.encode_fst4(ptr0, len0, ptr1, len1, ptr2, len2, freq_hz, submode);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v4 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v4;
}

/**
 * Encode an FT4 standard message (CALL1 CALL2 GRID/REPORT) as 12 kHz PCM.
 * @param {string} call1
 * @param {string} call2
 * @param {string} report
 * @param {number} freq_hz
 * @returns {Float32Array}
 */
export function encode_ft4(call1, call2, report, freq_hz) {
    const ptr0 = passStringToWasm0(call1, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(call2, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(report, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.encode_ft4(ptr0, len0, ptr1, len1, ptr2, len2, freq_hz);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v4 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v4;
}

/**
 * Encode a free-text FT4 message (up to 13 chars from the FT8 alphabet).
 * @param {string} text
 * @param {number} freq_hz
 * @returns {Float32Array}
 */
export function encode_ft4_free_text(text, freq_hz) {
    const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.encode_ft4_free_text(ptr0, len0, freq_hz);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v2;
}

/**
 * @param {string} call1
 * @param {string} call2
 * @param {string} report
 * @param {number} freq_hz
 * @returns {Float32Array}
 */
export function encode_ft8(call1, call2, report, freq_hz) {
    const ptr0 = passStringToWasm0(call1, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(call2, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(report, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.encode_ft8(ptr0, len0, ptr1, len1, ptr2, len2, freq_hz);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v4 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v4;
}

/**
 * Encode a standard Q65 message (`<call1> <call2> <grid_or_report>`)
 * at the requested sub-mode + audio centre frequency. Returns 12 kHz
 * f32 PCM at amplitude 0.3.
 * @param {string} call1
 * @param {string} call2
 * @param {string} grid_or_report
 * @param {number} freq_hz
 * @param {number} submode
 * @returns {Float32Array}
 */
export function encode_q65(call1, call2, grid_or_report, freq_hz, submode) {
    const ptr0 = passStringToWasm0(call1, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(call2, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(grid_or_report, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ret = wasm.encode_q65(ptr0, len0, ptr1, len1, ptr2, len2, freq_hz, submode);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v4 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v4;
}

/**
 * Encode a Type-1 WSPR message ("CALLSIGN GRID4 POWER_DBM") as 12 kHz
 * PCM audio suitable for transmission.
 * @param {string} callsign
 * @param {string} grid
 * @param {number} power_dbm
 * @param {number} freq_hz
 * @returns {Float32Array}
 */
export function encode_wspr(callsign, grid, power_dbm, freq_hz) {
    const ptr0 = passStringToWasm0(callsign, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(grid, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.encode_wspr(ptr0, len0, ptr1, len1, power_dbm, freq_hz);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    var v3 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
    wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
    return v3;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_344f42d3211c4765: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_call_a6e5c5dce5018821: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_decodedmessage_new: function(arg0) {
            const ret = DecodedMessage.__wrap(arg0);
            return ret;
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./ft8_web_bg.js": import0,
    };
}

const DecodedMessageFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_decodedmessage_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
    }
    wasm.__externref_drop_slice(ptr, len);
    return result;
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint16ArrayMemory0 = null;
function getUint16ArrayMemory0() {
    if (cachedUint16ArrayMemory0 === null || cachedUint16ArrayMemory0.byteLength === 0) {
        cachedUint16ArrayMemory0 = new Uint16Array(wasm.memory.buffer);
    }
    return cachedUint16ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function passArray16ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 2, 2) >>> 0;
    getUint16ArrayMemory0().set(arg, ptr / 2);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedUint16ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('ft8_web_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
