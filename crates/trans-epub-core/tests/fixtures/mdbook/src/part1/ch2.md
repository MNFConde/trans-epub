# Chapter 2

Rustdoc include with an item anchor:

```rust
{{#rustdoc_include code/item.rs:struct:Widget}}
```

Hidden lines demo:

```rust
fn visible() {}
{{#hide}}
{{#hide}}fn inline_hidden() {}
```
