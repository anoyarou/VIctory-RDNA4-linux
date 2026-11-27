//! What textures a draw reads at a pixel: `--reads <list>:<command>` with
//! `--pixel`. The title's shaders reach their textures through the heap by
//! indices they compute (bindless), so what a draw binds says little. Here
//! the heap slots of textures are made to read as a constant instead (a view
//! of the same texture, format and dimension whose channels are forced to 1,
//! then to 0: nothing else changes, so it is safe), in groups, and the draw's
//! targets under the points are compared with an unchanged run: a group that
//! changes them holds a slot the draw reads there, and is halved until the
//! slots are found. The frame runs up to the draw only (`--until`).
//!
//! Only the heap version the draw's submission uses is changed. Textures the
//! card writes in the frame (targets, shader-written textures) are left out
//! unless asked for, as are views made without a description.

use super::pixel::{Pixels, ROOM, Sample, texel_bytes, texel_text};
use super::*;
use crate::replay::edit::PROBE;

/// What to look for.
#[derive(Clone, Debug, Default)]
pub struct Reads {
    /// The draw: list, command.
    pub draw: (u32, u32),
    /// Textures the card writes in the frame too.
    pub all: bool,
}

/// A component mapping that gives `value` (0 or 1) in every channel.
fn forced(one: bool) -> u32 {
    let v = if one {
        D3D12_SHADER_COMPONENT_MAPPING_FORCE_VALUE_1.0
    } else {
        D3D12_SHADER_COMPONENT_MAPPING_FORCE_VALUE_0.0
    } as u32;
    v | v << 3 | v << 6 | v << 9 | 1 << 12
}

/// A heap slot that may be read: heap, index, view.
type Candidate = (u32, u32, Slot);

impl Player {
    /// The texture views of the heaps as the draws of version `version` see
    /// them.
    fn texture_slots(&self, version: usize, all: bool, skipped: &mut usize) -> Vec<Candidate> {
        let mut out = Vec::new();
        for h in &self.frame.descriptor_heaps {
            if h.kind == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER.0 {
                continue;
            }
            let mut now: HashMap<u32, Slot> = HashMap::new();
            for changes in self.changes.iter().take(version + 1) {
                for (id, i, s) in changes {
                    if *id == h.id && *i < h.count {
                        now.insert(*i, *s);
                    }
                }
            }
            let mut wanted = |i: u32, s: &Slot, out: &mut Vec<Candidate>| {
                if s.kind != slot::SRV || self.texture(s.res).is_none() {
                    return;
                }
                if !all && self.card_writes.contains(&s.res) {
                    return;
                }
                if s.described == 0 {
                    *skipped += 1;
                    return;
                }
                out.push((h.id, i, *s));
            };
            for (first, n, s) in &h.runs {
                for i in *first..first.saturating_add(*n).min(h.count) {
                    if !now.contains_key(&i) {
                        wanted(i, s, &mut out);
                    }
                }
            }
            let mut changed: Vec<(&u32, &Slot)> = now.iter().collect();
            changed.sort_by_key(|(i, _)| **i);
            for (i, s) in changed {
                wanted(*i, s, &mut out);
            }
        }
        out
    }

    /// Writes the views of `set` into version `version` of their heaps:
    /// forced to 1 or 0, or (`None`) as they were.
    fn knock(&self, set: &[Candidate], version: usize, value: Option<bool>) {
        for (h, i, s) in set {
            let Some((versions, step)) = self.descriptors.get(h) else {
                continue;
            };
            let Some(v) = versions.get(version) else {
                continue;
            };
            // SAFETY: a slot of a live heap the card is not using (the last
            // run was waited for); the slot's description is an SRV's.
            unsafe {
                let at = D3D12_CPU_DESCRIPTOR_HANDLE {
                    ptr: v.heap.GetCPUDescriptorHandleForHeapStart().ptr
                        + *i as usize * *step as usize,
                };
                match value {
                    Some(one) => {
                        let mut d = s
                            .desc
                            .as_ptr()
                            .cast::<D3D12_SHADER_RESOURCE_VIEW_DESC>()
                            .read_unaligned();
                        d.Shader4ComponentMapping = forced(one);
                        let res = self.objects.get(&s.res).map(|o| &o.res);
                        self.device.CreateShaderResourceView(res, Some(&d), at);
                    }
                    None => self.view(s, at, false),
                }
            }
        }
    }

    /// Says which texture slots change what draw `r.draw` writes under the
    /// points of `p`.
    pub fn reads(
        &mut self,
        o: &Options,
        p: &Pixels,
        r: &Reads,
        say: &mut dyn FnMut(String),
    ) -> Result<()> {
        let picture = self.picture_size().context("no picture was presented")?;
        let (list, command) = r.draw;
        self.set_merging(o.merge)?;
        let mut o = o.clone();
        o.until = Some((list, command as usize));
        let mut report = Report::default();
        let submits = self.edited(&o, &mut report, &[]);
        report.notes.drain(..).for_each(&mut *say);
        let mut first = 0u32;
        let mut submit = None;
        for (i, lists) in submits.iter().enumerate() {
            if list < first + lists.len() as u32 {
                submit = Some(i);
                break;
            }
            first += lists.len() as u32;
        }
        let submit = submit.with_context(|| format!("there is no list {list}"))?;
        let version = self.version_of.get(submit).copied().unwrap_or(0);
        let p = Pixels {
            lists: Some((list, list)),
            split: Vec::new(),
            ..p.clone()
        };
        let mut samples: Vec<Sample> = Vec::new();
        let made = self.sampled(&p, &submits, picture, &HashMap::new(), &mut samples)?;
        let mine: Vec<usize> = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.list == list && s.command == command as usize && !s.before)
            .map(|(i, _)| i)
            .collect();
        if mine.is_empty() {
            bail!(
                "{list}:{command} has no texel under the points to look at (a draw whose targets have the picture's shape?)"
            );
        }
        let len = samples.len() as u64 * ROOM;
        let (back, data) = gpu::buffer(
            &self.device,
            D3D12_HEAP_TYPE_READBACK,
            len.max(4),
            D3D12_RESOURCE_FLAG_NONE,
        )?;
        self.extra.insert(PROBE, back);
        let built = self.build(&made, &o)?;
        let restore = o.restore;
        let run = |this: &mut Player| -> Result<Vec<Vec<u8>>> {
            this.restore(restore)?;
            this.fresh = false;
            this.send(&built, &o)?;
            // SAFETY: the mapping of `len` bytes; the copies into it have run.
            let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
            Ok(mine
                .iter()
                .map(|&i| {
                    let n = texel_bytes(samples[i].spot.format).max(1);
                    bytes[i * ROOM as usize..][..n].to_vec()
                })
                .collect())
        };
        let base = run(self)?;
        let again = run(self)?;
        if base != again {
            say("the draw's texels differ between two unchanged runs: changes may be noise".into());
        }
        let mut skipped = 0;
        let all = self.texture_slots(version, r.all, &mut skipped);
        say(format!(
            "{list}:{command}: {} texels of its targets; {} texture slots in version {version} of the heaps{}{}",
            mine.len(),
            all.len(),
            if r.all { "" } else { " (not those the card writes in the frame)" },
            if skipped > 0 { format!("; {skipped} views without a description left out") } else { String::new() },
        ));
        let mut found: Vec<(usize, bool, Vec<Vec<u8>>)> = Vec::new();
        let mut runs = 2;
        for one in [true, false] {
            let mut stack: Vec<Vec<usize>> = vec![
                (0..all.len())
                    .filter(|i| !found.iter().any(|f| f.0 == *i))
                    .collect(),
            ];
            while let Some(set) = stack.pop() {
                if set.is_empty() {
                    continue;
                }
                let picked: Vec<Candidate> = set.iter().map(|&i| all[i]).collect();
                self.knock(&picked, version, Some(one));
                let got = run(self);
                self.knock(&picked, version, None);
                let got = got?;
                runs += 1;
                if got == base || got == again {
                    continue;
                }
                if let [only] = set[..] {
                    found.push((only, one, got));
                    continue;
                }
                let (a, b) = set.split_at(set.len() / 2);
                stack.push(b.to_vec());
                stack.push(a.to_vec());
            }
        }
        self.extra.remove(&PROBE);
        say(format!("{} slots change the texels ({runs} runs)", found.len()));
        found.sort_by_key(|f| (all[f.0].0, all[f.0].1));
        for (i, one, got) in &found {
            let (h, at, s) = &all[*i];
            // SAFETY: the slot's description is an SRV's, unaligned.
            let d = unsafe {
                s.desc
                    .as_ptr()
                    .cast::<D3D12_SHADER_RESOURCE_VIEW_DESC>()
                    .read_unaligned()
            };
            say(format!(
                "heap {h} slot {at}: srv f{} dim {} mapping {:#x}: {} (forced to {})",
                d.Format.0,
                d.ViewDimension.0,
                d.Shader4ComponentMapping,
                self.called(s.res),
                u8::from(*one),
            ));
            for (k, &n) in mine.iter().enumerate() {
                let t = &samples[n].spot;
                if got[k] == base[k] {
                    continue;
                }
                say(format!(
                    "    point {} {} at {},{}: {} -> {}",
                    t.point,
                    self.called(t.res),
                    t.x,
                    t.y,
                    texel_text(t.format, &base[k]),
                    texel_text(t.format, &got[k]),
                ));
            }
        }
        // The neighbourhood of what was found: slots the draw may mean to
        // read that hold no texture (empty, null, a buffer) cannot be found
        // above.
        // Runs of found slots no more than 8 apart, by heap.
        let mut near: Vec<(u32, u32, u32)> = Vec::new();
        let mut spots: Vec<(u32, u32)> = found.iter().map(|f| (all[f.0].0, all[f.0].1)).collect();
        spots.sort_unstable();
        for (h, at) in spots {
            match near.last_mut() {
                Some(last) if last.0 == h && at <= last.2 + 8 => last.2 = at,
                _ => near.push((h, at, at)),
            }
        }
        for (h, low, high) in near {
            let Some(heap) = self.frame.descriptor_heaps.iter().find(|x| x.id == h) else {
                continue;
            };
            let mut now: HashMap<u32, Slot> = HashMap::new();
            for changes in self.changes.iter().take(version + 1) {
                for (id, i, s) in changes {
                    if *id == h {
                        now.insert(*i, *s);
                    }
                }
            }
            say(format!("heap {h} slots {}..{} in version {version}:", low.saturating_sub(4), high + 4));
            for i in low.saturating_sub(4)..=(high + 4).min(heap.count.saturating_sub(1)) {
                let s = now.get(&i).copied().or_else(|| {
                    let k = heap.runs.partition_point(|run| run.0 <= i);
                    k.checked_sub(1)
                        .and_then(|k| heap.runs.get(k))
                        .filter(|(first, n, _)| i - first < *n)
                        .map(|r| r.2)
                });
                let text = match s {
                    None => "never written".to_owned(),
                    Some(s) => {
                        let kind = ["empty", "cbv", "srv", "uav", "sampler", "rtv", "dsv"]
                            .get(s.kind as usize)
                            .copied()
                            .unwrap_or("?");
                        let what = if s.res == NONE { "null".to_owned() } else { self.called(s.res) };
                        let changed = if now.contains_key(&i) { " (changed in the frame)" } else { "" };
                        format!("{kind} {what}{changed}")
                    }
                };
                let read = found.iter().any(|f| all[f.0].0 == h && all[f.0].1 == i);
                say(format!("    {i}{} {text}", if read { " read" } else { "" }));
            }
        }
        Ok(())
    }
}
