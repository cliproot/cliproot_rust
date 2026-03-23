# Cliproot Workflow Examples

Practical examples showing how to chain cliproot tools for common research tasks.

---

## Example 1: Capture-First Exploration

**Scenario**: Researching a new topic by capturing sources first, then synthesizing.

```
1. Capture key passages from multiple sources:

   cliproot_clip(url="https://example.com/article-1", quote="Redis uses an event loop...", id="redis-event-loop", source_type="external-quoted")
   cliproot_clip(url="https://example.com/article-2", quote="Memcached uses multi-threading...", id="memcached-threading", source_type="external-quoted")

2. Synthesize findings into a comparison:

   cliproot_derive(from=["redis-event-loop", "memcached-threading"], quote="Redis uses a single-threaded event loop while Memcached uses multi-threading. This means...", transformation_type="combine")

3. Verify all clips are intact:

   cliproot_verify()
```

---

## Example 2: Building a Knowledge Chain

**Scenario**: Progressive refinement from raw source to polished insight.

```
1. Capture the raw source:

   cliproot_clip(url="https://paper.example.com/ml-survey", quote="Transformer architectures have shown...", id="transformer-survey", source_type="external-quoted")

2. Summarize the key finding:

   cliproot_derive(from=["transformer-survey"], quote="Transformers outperform RNNs on sequence tasks due to...", transformation_type="summary")

3. Check the lineage:

   cliproot_trace(hash_or_id="transformer-survey")
```

---

## Example 3: Publishing with Citations

**Scenario**: Producing a final document with inline citations and bibliography.

```
1. Write your document using insights from captured clips.

2. Add inline citations:

   cliproot_annotate(document_text="<your document text>", style="footnote")

3. Generate bibliography:

   cliproot_cite(document_text="<your document text>")

4. Audit provenance coverage:

   cliproot_doctor(document_text="<your document text>")

5. Export the provenance bundle for a key clip:

   cliproot_export(hash_or_id="my-clip-id")
```

---

## Example 4: Building on Prior Work

**Scenario**: Continuing research that was started in a previous session.

```
1. Search for existing clips on the topic:

   cliproot_search(query="transformer")

2. List clips in a specific document:

   cliproot_list(document_id="ml-research-2025")

3. Inspect a specific clip for details:

   cliproot_inspect(hash_or_id="transformer-survey")

4. Derive new insights from existing clips:

   cliproot_derive(from=["transformer-survey", "attention-mechanism"], quote="Building on prior findings...", transformation_type="combine")
```
