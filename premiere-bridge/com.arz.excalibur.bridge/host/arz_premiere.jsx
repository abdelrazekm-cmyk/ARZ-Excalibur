// ARZ-Excalibur — Clean-Room Adobe Premiere Pro ExtendScript Host Layer
// 100% readable source, zero license checks, zero external telemetry.

if (typeof $ === "undefined") {
    $ = {};
}

$.arzExcalibur = (function () {
    var qeEnabled = false;

    function ensureQE() {
        if (!qeEnabled) {
            app.enableQE();
            qeEnabled = true;
        }
        return qe;
    }

    function applyMath(currentVal, op, val) {
        if (op === "set") return val;
        if (op === "add") return currentVal + val;
        if (op === "sub") return currentVal - val;
        if (op === "mul") return currentVal * val;
        if (op === "div") return val !== 0 ? currentVal / val : currentVal;
        if (op === "percent") return currentVal * (val / 100.0);
        return val;
    }

    function getSelectedClips(seq) {
        if (!seq) return [];
        var sel = seq.getSelection();
        return sel || [];
    }

    function setClipProperty(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var clips = getSelectedClips(seq);
        var playhead = seq.getPlayerPosition();
        var updated = 0;

        for (var i = 0; i < clips.length; i++) {
            var clip = clips[i];
            for (var c = 0; c < clip.components.numItems; c++) {
                var comp = clip.components[c];
                if (comp.displayName === cmd.component || comp.matchName === cmd.component) {
                    for (var p = 0; p < comp.properties.numItems; p++) {
                        var prop = comp.properties[p];
                        if (prop.displayName === cmd.param) {
                            if (cmd.op === "toggle_keyframe") {
                                if (!prop.isTimeVarying()) {
                                    prop.setTimeVarying(true);
                                }
                                prop.addKey(playhead);
                                updated++;
                            } else {
                                var cur = prop.getValue();
                                if (cur instanceof Array && typeof cmd.axis === "number") {
                                    cur[cmd.axis] = applyMath(Number(cur[cmd.axis]), cmd.op, Number(cmd.value));
                                    prop.setValue(cur, true);
                                } else {
                                    var nextVal = applyMath(Number(cur), cmd.op, Number(cmd.value));
                                    prop.setValue(nextVal, true);
                                }
                                updated++;
                            }
                        }
                    }
                }
            }
        }
        return { ok: true, updated: updated };
    }

    function forEachSelectedQEClip(isVideo, fn) {
        var q = ensureQE();
        var qeSeq = q.project.getActiveSequence();
        if (!qeSeq) return 0;
        var domSeq = app.project.activeSequence;
        if (!domSeq) return 0;

        var domTracks = isVideo ? domSeq.videoTracks : domSeq.audioTracks;
        var count = 0;
        for (var t = 0; t < domTracks.numTracks; t++) {
            var dTrack = domTracks[t];
            var qTrack = isVideo ? qeSeq.getVideoTrackAt(t) : qeSeq.getAudioTrackAt(t);
            for (var k = 0; k < dTrack.clips.numItems; k++) {
                var dClip = dTrack.clips[k];
                if (dClip.isSelected()) {
                    for (var qk = 0; qk < qTrack.numItems; qk++) {
                        var qItem = qTrack.getItemAt(qk);
                        if (qItem && qItem.start && qItem.start.ticks === dClip.start.ticks) {
                            fn(qItem, dClip);
                            count++;
                            break;
                        }
                    }
                }
            }
        }
        return count;
    }

    function applyEffect(cmd) {
        var q = ensureQE();
        var effectObj = cmd.is_video
            ? q.project.getVideoEffectByName(cmd.name)
            : q.project.getAudioEffectByName(cmd.name);
        if (!effectObj) return { ok: false, error: "Effect not found: " + cmd.name };

        var applied = forEachSelectedQEClip(cmd.is_video, function (qClip) {
            if (cmd.is_video) {
                qClip.addVideoEffect(effectObj);
            } else {
                qClip.addAudioEffect(effectObj);
            }
        });
        return { ok: true, applied: applied };
    }

    function applyTransition(cmd) {
        var q = ensureQE();
        var transObj = cmd.is_video
            ? q.project.getVideoTransitionByName(cmd.name)
            : q.project.getAudioTransitionByName(cmd.name);
        if (!transObj) return { ok: false, error: "Transition not found: " + cmd.name };

        var applied = forEachSelectedQEClip(cmd.is_video, function (qClip) {
            if (cmd.is_video) {
                qClip.addVideoTransition(transObj);
            } else {
                qClip.addAudioTransition(transObj);
            }
        });
        return { ok: true, applied: applied };
    }

    function applyCompiledPreset(cmd) {
        var preset = cmd.preset;
        if (!preset || !preset.effects) return { ok: false, error: "Invalid preset" };
        var q = ensureQE();

        var applied = forEachSelectedQEClip(preset.is_video, function (qClip, dClip) {
            for (var e = 0; e < preset.effects.length; e++) {
                var fx = preset.effects[e];
                var fxObj = preset.is_video
                    ? q.project.getVideoEffectByName(fx.display_name)
                    : q.project.getAudioEffectByName(fx.display_name);
                if (fxObj) {
                    if (preset.is_video) qClip.addVideoEffect(fxObj);
                    else qClip.addAudioEffect(fxObj);
                }

                // Apply parameter values onto newly added component
                for (var c = dClip.components.numItems - 1; c >= 0; c--) {
                    var comp = dClip.components[c];
                    if (comp.matchName === fx.match_name || comp.displayName === fx.display_name) {
                        for (var pi = 0; pi < fx.params.length; pi++) {
                            var pRecipe = fx.params[pi];
                            for (var dp = 0; dp < comp.properties.numItems; dp++) {
                                if (comp.properties[dp].displayName === pRecipe.name) {
                                    try {
                                        comp.properties[dp].setValue(pRecipe.current_value, true);
                                    } catch (_e) {}
                                    break;
                                }
                            }
                        }
                        break;
                    }
                }
            }
        });
        return { ok: true, applied: applied };
    }

    function trimToPlayhead(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var playhead = seq.getPlayerPosition();
        var clips = getSelectedClips(seq);
        var updated = 0;

        for (var i = 0; i < clips.length; i++) {
            var clip = clips[i];
            if (Number(playhead.ticks) > Number(clip.start.ticks) && Number(playhead.ticks) < Number(clip.end.ticks)) {
                if (cmd.trim_in) {
                    var deltaSeconds = playhead.seconds - clip.start.seconds;
                    var newIn = new Time();
                    newIn.seconds = clip.inPoint.seconds + deltaSeconds;
                    clip.inPoint = newIn;
                    clip.start = playhead;
                } else {
                    clip.end = playhead;
                }
                updated++;
            }
        }
        return { ok: true, updated: updated };
    }

    function moveClipToPlayhead(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var playhead = seq.getPlayerPosition();
        var clips = getSelectedClips(seq);
        for (var i = 0; i < clips.length; i++) {
            var dur = clips[i].duration.seconds;
            if (cmd.align_start) {
                clips[i].move(playhead.seconds - clips[i].start.seconds);
            } else {
                clips[i].move((playhead.seconds - dur) - clips[i].start.seconds);
            }
        }
        return { ok: true, updated: clips.length };
    }

    function nestClips(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var subSeq = seq.createSubsequence(true);
        if (subSeq && cmd.sequence_name) {
            subSeq.name = cmd.sequence_name;
        }
        return { ok: true };
    }

    function addMarker(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var playhead = seq.getPlayerPosition();
        if (cmd.on_clip) {
            var clips = getSelectedClips(seq);
            for (var i = 0; i < clips.length; i++) {
                var m = clips[i].projectItem.getMarkers().createMarker(clips[i].inPoint.seconds);
                m.name = cmd.name || "ARZ Marker";
                m.setColorByIndex(cmd.color_index || 0);
            }
        } else {
            var sm = seq.markers.createMarker(playhead.seconds);
            sm.name = cmd.name || "ARZ Marker";
            sm.setColorByIndex(cmd.color_index || 0);
        }
        return { ok: true };
    }

    function movePlayhead(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var cur = seq.getPlayerPosition();
        var frameTicks = Number(seq.timebase);
        var nextTicks = Math.max(0, Number(cur.ticks) + Number(cmd.delta_frames) * frameTicks);
        seq.setPlayerPosition(String(nextTicks));
        return { ok: true };
    }

    function selectClipsRelative(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var playheadTicks = Number(seq.getPlayerPosition().ticks);
        var groups = [seq.videoTracks, seq.audioTracks];
        for (var g = 0; g < groups.length; g++) {
            for (var t = 0; t < groups[g].numTracks; t++) {
                var trk = groups[g][t];
                for (var c = 0; c < trk.clips.numItems; c++) {
                    var clip = trk.clips[c];
                    var hit = cmd.after
                        ? Number(clip.start.ticks) >= playheadTicks
                        : Number(clip.end.ticks) <= playheadTicks;
                    clip.setSelected(hit, true);
                }
            }
        }
        return { ok: true };
    }

    function setTracksState(cmd) {
        var seq = app.project.activeSequence;
        if (!seq) return { ok: false, error: "No active sequence" };
        var tracks = cmd.track_kind === "audio" ? seq.audioTracks : seq.videoTracks;
        for (var t = 0; t < tracks.numTracks; t++) {
            if (cmd.field === "mute") {
                var nextMute = cmd.toggle ? (tracks[t].isMuted() ? 0 : 1) : 1;
                tracks[t].setMute(nextMute);
            }
        }
        return { ok: true };
    }

    function capturePreviewFrame(cmd) {
        var domSeq = app.project.activeSequence;
        if (!domSeq) return { ok: false, error: "No active sequence" };
        var q = ensureQE();
        var qeSeq = q.project.getActiveSequence();
        if (!qeSeq || !qeSeq.exportFramePNG) return { ok: false, error: "QE exportFramePNG unavailable" };
        var ticks = domSeq.getPlayerPosition().ticks;
        var ok = qeSeq.exportFramePNG(ticks, cmd.output_path);
        return { ok: Boolean(ok), path: cmd.output_path };
    }

    function dispatch(jsonStr) {
        try {
            var cmd = JSON.parse(jsonStr);
            var out;
            switch (cmd.action) {
                case "capture_preview_frame": out = capturePreviewFrame(cmd); break;
                case "set_clip_property": out = setClipProperty(cmd); break;
                case "apply_effect": out = applyEffect(cmd); break;
                case "apply_transition": out = applyTransition(cmd); break;
                case "apply_compiled_preset": out = applyCompiledPreset(cmd); break;
                case "trim_to_playhead": out = trimToPlayhead(cmd); break;
                case "move_clip_to_playhead": out = moveClipToPlayhead(cmd); break;
                case "nest_clips": out = nestClips(cmd); break;
                case "add_marker": out = addMarker(cmd); break;
                case "move_playhead": out = movePlayhead(cmd); break;
                case "select_clips_relative_to_playhead": out = selectClipsRelative(cmd); break;
                case "set_tracks_state": out = setTracksState(cmd); break;
                case "run_jsx_file": $.evalFile(cmd.path); out = { ok: true }; break;
                default: out = { ok: false, error: "Unknown action: " + cmd.action };
            }
            return JSON.stringify(out);
        } catch (err) {
            return JSON.stringify({ ok: false, error: String(err) });
        }
    }

    return { dispatch: dispatch };
})();
