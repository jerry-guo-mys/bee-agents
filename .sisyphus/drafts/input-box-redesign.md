# Draft: Input Box Redesign

## Current Implementation (现状)

**Files**:
- `src/ui/app.rs` - 输入缓冲处理 (input_buffer), 键盘事件处理
- `src/ui/render.rs` - 输入框渲染 (Paragraph widget with Block)
- `src/ui/event.rs` - 快捷键事件分发

**Current Design**:
- Simple Paragraph widget with Block border
- Title shows "输入" or error state
- Bottom hint: "Enter 发送 │ ↑↓ PgUp/PgDn 滚动 │ Ctrl+C 取消 │ Ctrl+Q 退出"
- No dropdown selectors
- No model selection display
- No image upload button
- Single line input (5 rows height but no multi-line editing)

## Target Design (参考图片)

**Components needed**:
1. **Main input area** - Multi-line text input with placeholder
2. **Agent selector dropdown** - "Prometheus (Plan Builder)" with chevron
3. **Model selector dropdown** - "Gemini 3 Pro Preview" with sparkle icon + chevron  
4. **Mode selector** - "默认" (default mode)
5. **Image upload button** - Image icon button
6. **Send button** - Up arrow icon button (disabled state when empty)

**Visual style**:
- Rounded corners
- Light gray border
- Subtle shadows
- Clean, modern look
- Icons for actions

## Key Differences (Gap Analysis)

| Feature | Current | Target |
|---------|---------|--------|
| Dropdowns | None | 2 (Agent + Model) |
| Mode selector | None | Yes ("默认") |
| Image upload | No | Yes |
| Send button | Enter key only | Visual button + Enter |
| Placeholder | No | Yes ("随便问点什么...") |
| Styling | Basic ratatui Block | Modern rounded design |
| Icons | None | Sparkle, Image, Arrow |

## Technical Considerations

**Ratatui limitations**:
- No native dropdown component - need custom implementation
- Need to handle focus states for dropdowns
- Need keyboard navigation for dropdown menus
- Icon rendering in terminal (unicode symbols or ASCII art)

**Possible approaches**:
1. Use `ratatui-extras` or community widgets if available
2. Implement custom dropdown with Popup/Select widgets
3. Use unicode characters for icons (◆ for sparkle, 🖼️ for image, ↑ for send)
4. Consider using `tui-input` crate for better input handling

## Open Questions

1. **Dropdown behavior**: Click to open? Keyboard navigation? Both?
2. **Agent options**: What agents are available? (Prometheus, Sisyphus, etc.)
3. **Model options**: What models to show? (DeepSeek, OpenAI, Mock?)
4. **Mode options**: What modes beyond "默认"?
5. **Image upload**: Actual file picker or path input?
6. **Priority**: Which features are MVP vs nice-to-have?

## Scope Decision Needed

User should clarify:
- Must-have features vs nice-to-have
- Whether to use existing ratatui widgets or implement from scratch
- Icon strategy (unicode vs ASCII)
- Dropdown interaction pattern
