# Roblox GUI internals — answers from the 2022 decompile

**Source**: `2022-full.tar.xz` → "2022 Studio + full PDB" (IDA decompile, tier 3). Every file belongs to this single build (per `SOURCE.md`). Function citations below are **name + address in this build**. Main files:

| File | Class |
|---|---|
| `v8datamodel/BillboardGui.c` | BillboardGui |
| `v8datamodel/SurfaceGui.c` + `rbx/SurfaceGuiBase.c` | SurfaceGui / SurfaceGuiBase |
| `rbx/AdGui.c` | AdGui (SurfaceGuiBase subclass, immersive ads) |
| `v8datamodel/GuiLayerCollector.c` | GuiLayerCollector |
| `v8datamodel/GuiObject.c` | GuiObject |
| `v8datamodel/ScreenGui.c`, `v8datamodel/GuiBase2d.c` | ScreenGui / GuiBase2d |
| `rbx/BasePlayerGui.c`, `rbx/DisplayOrderComparator.c` | cross-collector ordering |
| `rbx/CanvasGroup.c`, `v8datamodel/ViewportFrame.c` | CanvasGroup / ViewportFrame |
| `free/_free_l.c` | free functions (loadDescendants…) |

**Class lineage (verified via ClassDescriptor bases & ctor vtables)**
`Instance → GuiBase → GuiBase2d → GuiLayerCollector → { ScreenGui, BillboardGui }` and `GuiLayerCollector → SurfaceGuiBase → { SurfaceGui, AdGui }`. `GuiBase2d → GuiObject → { GuiButton, GuiLabel, CanvasGroup, ViewportFrame, ScrollingFrame, … }`.
GuiLayerCollector vtables include `IAdornable`, `IStepped`, `HeartbeatInstance` (visible in `BillboardGui::BillboardGui`, 0x141676130).

> Caveat on decompile artifacts: struct-field *names* come from the PDB, but IDA sometimes assigns a plausible-but-wrong field name to an offset (flagged inline below). Offsets like "player character at +264" are exact-but-anonymous.

---

## 1. BillboardGui — eligibility and projection

### Render/input eligibility chain
* `stepLayouts` (0x1416793f0, runs per-frame on `Stepped`): recomputes the projection **every frame** when `enabled && viewportFrame == nullptr`:
  `visibleAndValid = getBillboardCFrame(projectionFrame, viewport, *outDistance)`; on success calls `GuiLayerCollector::setViewport`. So "is the billboard currently valid" = `visibleAndValid`.
* `getBillboardCFrame` (0x1416768e0) returns **false** (not eligible) unless:
  1. `getPart` (0x1416778e0) resolves an adornee (explicit `Adornee` weak ref, else ancestor instance).
  2. Adornee class determines the anchor: `BasePart → getCoordinateFrame + getPhysicalSize`; `Model → calculateModelCFrame/calculateModelSize`; `Attachment → getFrameInWorld`; anything else → identity CFrame with zero extents.
  3. A `Workspace` exists; camera `getRenderingCoordinateFrame` available.
  4. **MaxDistance test**: skipped entirely when `maxDistance <= 0` (ctor default is `INFINITY`, see 0x141676239); otherwise the camera-space depth must satisfy `-maxDistance <= depth` (i.e. within MaxDistance).
  5. **Projection depth test**: after computing the world-space point and calling `Camera::project`, the projected z must be in **`(0, 1000]`** — behind the camera or >1000 studs away ⇒ not eligible. (VR path bypasses this with a fixed `z = 20.0f`, 0x41A00000.)
* VR: if `UserInputService.VREnabled`, the camera frame is rebuilt with `CoordinateFrame::lookAt` mirrored across the camera position; further split on `FFlag::UserFlagEnableNewVRSystem`.
* Billboard center math: `center = lerp(extentMin, extentMax, (partExtentRelativeOffset.axis + 1) * 0.5) + partStudsOffset.axis` — i.e. **ExtentsOffset components are in [-1, 1] mapped onto the part extents**, then `StudsOffset` (part-relative) and `StudsOffsetWorldSpace`/`ExtentsOffsetWorldSpace` (camera-rotation applied; extents scaled by half adornee size) are added.
* Pixel size: `RoundToScreenScale(billboardSize (UDim2) * projectedZ)`; `translationFromScreenCenterPx` is stored on the object; result CFrame = `cameraCF * (pixelOffset / z)` scale-baked frame.
* `process` (input, 0x141678280): bails unless `enabled && active && visibleAndValid && viewportFrame == nullptr`; only mouse (≤ TYPE_MOUSEMOVEMENT) and input types 5–7 (touch) are considered; builds a camera `worldRay` from the 2D position (VR: ray from `VRService.userCFrame[guiInputUserCFrame]` under `FFlag::UserFlagEnableVRUpdate2`).
* `getInputPositionOffset` (0x141677840) → `getScreenPositionOffset` (0x141677b60) only for mouse/touch, identity for other types. `getGuiObjectsAtPosition` on the collector calls this virtual, so 3D guis answer position queries in adorn space.
* `getScreenSpaceBounds` (0x141677dd0): requires `enabled && visibleAndValid && viewportFrame == nullptr`, else throws `"Can't get screen bounds because no adornee or workspace"`.

### 3D-adorn render eligibility (Studio/player-facing)
`shouldRender3dAdorn` (0x141679310): needs part + DataModel + no active adorn recording + GuiService; then:
- if `GuiService->guiVisibilityByType[2]` (Custom-billboards visibility) → render;
- else render only if **not** "adorned to a player" — `calculateIsAdornedToPlayer` (0x141676540) iterates `Players` connected players and tests whether the adornee (or the gui's own parent) is inside any player's character.

`render3dAdorn` (0x141678970): skips entirely when the stored `playerToHideFrom` instance is the local player (play mode); otherwise renders the 2D scene into the adorn with `adorn->currentSmoothScaling = true`.

### Defaults & Studio property hiding
`BillboardGui()` (0x141676130): `maxDistance = INFINITY`, `distanceUpperLimit = -1`, `distanceLowerLimit = 0`, `currentDistance = 0`, `alwaysOnTop = false`, `clipping = false`, `visibleAndValid = false`.
`isPropertyHiddenInStudio` (0x141678150): `Brightness` hidden unless `FFlag::GuiBrightness && !alwaysOnTop && lightInfluence != 1.0`; `DistanceLowerLimit / DistanceUpperLimit / DistanceStep / CurrentDistance` are **always hidden in Studio**.

---

## 2. SurfaceGui behavior

* `SurfaceGuiBase::updateViewport` (0x141c2abc0): in `SIZING_PIXELSPERSTUD` (PixelsPerStud mode) the viewport pixel size = the face's physical 2D extent × `pixelsPerStud`. Face→axis mapping: X faces → (size.z, size.y); Y faces → (size.z, size.x); Z faces → (size.x, size.y). In `SIZING_FIXED` the fixed `canvasSize` is used. `setViewport` propagates to the GuiLayerCollector.
* Face projection matrix: `GuiBase2d::buildProjectedGuiMatrix` (0x14161cec0) — takes part CFrame + **getVisualSize** (render size, not physical), applies per-face yaw/pitch rotation tables indexed by NormalId, scales by `1/canvasSize`, recenters with offset `(-0.5, 0.5, 0.5)`, multiplies by part size. Used for both rendering and input mapping. Returns visibility-fail when the face extents fail the adorn frustum check.
* `SurfaceGuiBase::process` (0x141c29c70): **gamepad events are rerouted to plain 2D processing** (`GuiLayerCollector::process`). Otherwise: gates on `enabled` (+ a field read the decompiler labeled `alwaysOnTop` — treat as suspect, likely a mislabeled flag, and `viewportFrame == nullptr`), requires workspace + camera; then `BasePart::getPlaneByFaceNormal(faceID, useRenderCFrame = true)`, `Camera::worldRay` from the mouse position, `Ray::intersectionPlane`; an Inf/NaN intersection ⇒ no response; success ⇒ `process3d(point, ignoreMaxDistance=false)`.
* `process3d` (0x141c296e0):
  - gates `enabled && active && viewportFrame == nullptr`;
  - if `buildProjectedGuiMatrix` fails → `resolveInputOver(emptySet, event)` — **hover is actively cleared**;
  - **ToolPunchThroughDistance**: finds local character (`Players::findLocalCharacter`), reads its root position; if `toolPunchThroughDistance > 0 && !ignoreMaxDistance` and `distance(hitPoint, characterPos) >= toolPunchThroughDistance` ⇒ input is refused (the tool/world gets it instead). No local character ⇒ check never applies (edit mode);
  - input filter: only mouse-types, `TYPE_TOUCH`, or gamepad when `FFlag::UserFlagEnableVRUpdate2`;
  - inverts the projection `Matrix3` (`FFlag::FixSmallSurfaceGuiMouseInput` switches inverse tolerance between `0.0` and `1e-6`), maps hit point → 2D surface pixel, **temporarily rewrites `InputObject::positionOffset` to the surface-space 2D point**, runs `GuiLayerCollector::processWithMouseOverOption(event, sinkIfMouseOver=false)`, then **restores the original position**. Children therefore see input already expressed in SurfaceGui pixels.
* AdGui (`rbx/AdGui.c`, subclass of SurfaceGuiBase per 0x141d62540) adds the immersive-ad eligibility family: `validate`, `forceInvalid/removeForceInvalid`, `hasOtherSurfaceGuiOnPartFace`, `checkAndMarkVisible`, `fetchAd/disableFetch`, `onNoFill`, `onReceiveAdData`, `onHeartbeat`. (Say the word if you want those bodies decoded too.)

---

## 3. LayerCollector traversal (Z-order)

`GuiLayerCollector::loadZVectors` (0x14162c290, only when `rebuildGuiVector` dirty): clears the `UIQuadTree` + `renderOrder` map, rebuilds `guiVector`, then (ScreenGui root) the quadtree itself is rebuilt in `processDescendants` with bounds = viewport + GuiService inset, `maxDepth = 3`.

Two traversal modes, selected by `ZIndexBehavior`:

* **Global** — `loadDescendantsGlobalStructured` (0x14162b9e0): depth-first pre-order walk using `getReplicatedInsertionOrderSortedChildren` at each level; every `GuiBase` descendant is pushed into one flat vector; after the walk the **whole vector gets one `std::stable_sort` with `CompareOnlyZIndex`**.
* **Sibling** — `loadDescendantsSiblingStructured` (0x14162bbb0): per-generation — `loadGeneration` (direct GuiBase children), `stable_sort` that generation by ZIndex, append, then **recurse into each child in that sorted order**. Sibling subtrees therefore stay contiguous: a parent's entire subtree is emitted before the next sibling regardless of ZIndex.

`CompareOnlyZIndex` compares **`getZIndex()` only** (see the inlined predicate in `_Insertion_sort_unchecked<…,CompareOnlyZIndex>`, 0x141624660). Because the sort is stable, ties fall back to pre-sort order — replicated insertion order in Global mode; tree order in Sibling mode. (`CompareSiblingsOnlyInsertionOrder` is the sibling insertion-order comparator used by the replicated-order children getter.)

`setZIndexBehavior` (0x141630710) just raises the property change, which marks the vector dirty. Trivia: the rebuild profiler token is literally named `"Rebuild Z-order list"`.

---

## 4. GuiObject eligibility (who may process input)

* `canProcessMeAndDescendants` (0x141662f40) is literally `return this->getVisible();` — `Visible=false` ⇒ the whole subtree is **skipped for input** (checked first in `processItem`, 0x14162e990).
* `processItem`: skip if `!canProcessMeAndDescendants()` or the running answer is already `SUNK` (short-circuit); else call the virtual `process(event)`; on `SUNK` propagate response/finished/target; on `MOUSE_OVER` record the first mouse-over target.
* `processTouchEvent` (0x14166c070): `isOver(event) && active` ⇒ **`SUNK` + finished**; otherwise NOT_SUNK. So on touch, any Active GuiObject under the finger sinks. (Also `active && draggable` ⇒ `handleDragging`.)
* `processMouseEventInternal` (0x14166b6a0):
  - `isOver` gates everything; fires `mouseMovedSignal(x,y)` / `mouseWheelForward/BackwardSignal` (wheel z > 0 = forward);
  - dragging only when `active && draggable && UserInputService.MouseEnabled && !TouchEnabled`;
  - touch-originated input checks the nearest `ScrollingFrame` ancestor and suppresses press-state changes while touch/inertial scrolling;
  - drives the `guiState` machine (Idle=0 / Hover=1 / Press=2 with `lastMouseDownType` tracking), dirtying the ancestor collector (`GuiLayerCollector::markAppearanceDirty`, gated by `FFlag::ReduceGuiStateGfxGuiInvalidation`);
  - **final answer: if `!active` → NOT_SUNK + NOT_MOUSE_OVER; else NOT_SUNK + MOUSE_OVER + target = self.** The base GuiObject never SINKs mouse input by itself — GuiButton subclasses override `process` to sink clicks.
* `processKeyboardEvent`-level handling lives in the collector: `Enter` (keyCode 13) goes **directly to `GuiService::selectedGuiObject`**; other keys are fanned out to hovered objects (see §5).
* Collector-level gates for 3D guis: `enabled && active && visibleAndValid && viewportFrame == nullptr` (Billboard) / `enabled && active && viewportFrame == nullptr` (SurfaceGui process3d).

---

## 5. Input ordering (cross-collector and within a collector)

### Across layer collectors — `BasePlayerGui`
* `doRebuildRenderAndInputLists` (0x141484c40) maintains one **`stable_sort` of the PlayerGui's children with `DisplayOrderComparator`** (rebuilt whenever a descendant DisplayOrder/etc. changes; has a `DisplayOrderComparator_DEPRECATED` fallback path).
* `DisplayOrderComparator::createSortKey` (0x141483720) produces tuple keys `<sortOrder, displayOrder, depth>`:
  - **ScreenGui** → `(sortOrder = 3 + (OnTopOfCoreBlur ? 1 : 0), displayOrder, 0.0)`;
  - **BillboardGui** → `(sortOrder = 1 + (AlwaysOnTop ? 1 : 0), displayOrder, −cameraDot(adorneePos))` — the negated camera depth means nearer billboards sort later/earlier consistently.
* `getOrderedChildList` (0x141487070): under `FFlag::SortBillboardGuisForInput` it re-sorts BillboardGuis using the camera coordinate frame for the input pass.
* `BasePlayerGui::process` (0x141487df0): iterates the ordered list **in reverse (back → front, i.e. topmost/highest-sort first)**, calls each child's virtual `process`, and the **first `SUNK` answer wins and stops the loop** (result = SUNK, mouseWasOver = MOUSE_OVER, target propagated). Render order and input order are two ends of the same sorted list.

### Within one collector — `GuiLayerCollector`
* `process` (0x14162d4b0) → `processWithMouseOverOption(event, sinkIfMouseOver=true)`, then computes `forceArrowCursor` (= SUNK hit something that isn't the default cursor owner).
* `processDescendants` (0x14162d5a0):
  - **Mouse/touch inputs**: `UIQuadTree::query(position)` returns the hit `GuiUpdateStruct` vector; iterates it **backwards (topmost Z first)** calling `processItem` (§4) until SUNK; every GuiObject whose `isOver(event)` virtual returns true is appended to `newElementsOverSet` with an `inputWasSunk` flag. Afterwards `resolveInputOver` (0x14162f910) diffs the new set against `oldInputOverObjectsMap` (keyed by InputObject) to emit MouseEnter/MouseLeave and store the per-input hover set (`lockUIQuadTree` toggling gated by `FFlag::AvoidUnnecessaryQuadtreeLock`).
  - **Keyboard**: `Enter` → selected object only (above). Otherwise the quadtree is queried at the input position and the hit vector walked backwards, firing that GuiObject's `InputBegan`/`InputEnded` rbx::signal directly.
  - **Everything else (gamepad & non-positional)**: iterates `uiQuadTree->renderVector` (the full render-order list) instead of a point query.
* `updateInputOverGuiObjects` (0x141630dc0) recomputes the hover set for a given InputObject on demand (e.g. after layout moves) using `isOver(pos)` retests, firing `InputEnded` on objects that fell out.
* Public queries: `getGuiObjects(event)` (0x14162aa80) returns the stored hover vector for that input; `getGuiObjectsAtPosition(pos, radius)` (0x14162ac20) applies the virtual `getScreenPositionOffset` (billboards/3D aware) then queries the quadtree; `PlayerModule`-facing wrappers live in `BasePlayerGui` (`getGuiObjectsAtPosition(Lua)`, `getGuiObjectsInCircle`).

Resolved ordering, end to end: **PlayerGui children by (sortOrder, DisplayOrder, depth) stable — reverse traversal → first collector to SUNK wins → inside the collector, topmost-Z-first via quadtree/render vector; sibling ties keep insertion order.**

---

## 6. CanvasGroup

* `render2d` (0x141898880, under `FFlag::RenderCanvasGroupToTexture`):
  1. Background: `getRenderBackgroundColor4` with alpha premultiplied by `1 − GroupTransparency` ("gtAppliedBgColor"), border via `GuiObject::render2dBorder` with that color.
  2. If `GroupTransparency < 1.0`: computes the screen rect (`RoundToScreenScale` on both corners), then calls `Adorn::requestOrUpdateUIRT(guiRTCache, RenderTargetConfiguration{rect, pixel size, …})`, `Adorn::setRenderTarget(rtId)`, and blits the composed group texture with `Adorn::rect2d(rect, uv 0..1, color = GroupColor3 × alpha(1 − GroupTransparency))` — i.e. **descendants render into an offscreen UI render target, and the whole group is re-composited to screen with GroupTransparency/GroupColor applied uniformly**.
  3. Clipping against `firstAncestorClipping()->getClippedRect()` gates the blit.
* `isPropertyHiddenInStudio` (0x1418982d0): the **`Clipping` (ClipsDescendants) property is always hidden in Studio** for CanvasGroup — canvas groups clip unconditionally (`setClippingInternal` 0x… in same file).
* `getOverrideInstances` (0x141898220): `GroupColor` / `GroupTransparency` mark the ancestor `GuiLayerCollector` with override status 2 **when `ZIndexBehavior != Sibling`** — i.e. the property is flagged unsupported under Global z-index behavior.

---

## 7. ViewportFrame

* Properties registered in this build: `Ambient`, `CameraCFrame`, `CameraFieldOfView`, `CurrentCamera`, `ImageColor3`, `ImageTransparency`, `IsMirrored`, `LightColor`, `LightDirection` (dynamic initializers 0x1402e91c0–0x1402e9dd0). `setCurrentCamera` (0x141989650) manages a `cameraPropertyConnection` (re-hooks camera property signals on change).
* `render2d` (0x141989110):
  - unless `FFlag::ViewportFrameUseRender2dBorder2`, draws the legacy border: `outlineRect2d` with the render background color (border thickness scaled by `getTotalGroupScale()` when > 0), respecting `firstAncestorClipping()->getClippedRect()` when unrotated;
  - **mirroring**: under `DFFlag::ViewportFrameEnableMirroring`, UVs flip horizontally when `IsMirrored` (`uvtl.x = mirrored ? 1 : 0`, `uvbr.x = mirrored ? 0 : 1`);
  - then blits the viewport's rendered texture over the GuiObject rect tinted by `ImageColor3` with alpha `1 − ImageTransparency`.
* Everywhere else in the GUI system, being inside a ViewportFrame **disables input**: `@property viewportFrame != nullptr` is a hard bail-out in `BillboardGui::process/stepLayouts/getScreenSpaceBounds` and `SurfaceGuiBase::process/process3d`. `findFirstAncestorOfType<ViewportFrame>` (0x140c5cfa0) is the lookup used for that flag.
* Note: **no `WorldModel` references appear in this build's `ViewportFrame.c`** — the viewport content path here is plain PVInstance children + the frame's own camera, rendered to the RT that `render2d` blits.

---

## 8. Studio edit mode vs play mode (where they differ)

1. **Billboard 3D-adorn suppression depends on players** — `calculateIsAdornedToPlayer` iterates connected players/characters (0x141676540). In **edit mode there are no player characters**, so `isAdornedToPlayer` is false and billboards render in the 3D viewport even when `guiVisibilityByType[2]` is off; in **play/test mode** the same configuration suppresses billboards attached to characters.
2. **`PlayerToHideFrom` only takes effect with a local player** — `render3dAdorn` (0x141678970) searches for the local player and skips rendering when matched. Edit mode (no local player) always renders.
3. **`ToolPunchThroughDistance` requires a local character** (play mode) — edit mode never applies the distance gate for SurfaceGui input.
4. **GuiService per-type visibility** (`guiVisibilityByType[4]`, default `0x01010101`): `[0]=Custom, [1]=RobloxGui, [2]=CustomBillboards, [3]=RobloxBillboards`. `getGuiIsVisible` 0x1412ada60 / `toggleGuiIsVisible` 0x1414bb770 / `toggleGuiIsVisibleIfAllowed` 0x1414bb930. `Workspace::render3dAdorn` gates the whole 3D-adorn GUI pass on `[3]` (0x1412dc570); BillboardGui's own check reads `[2]`. Toggled identically in both modes, but 2D CoreGui/player 2D content only exists under a run DataModel while 3D adorns render in edit mode too.
5. **Studio-only decoration path** — `GuiObject::renderStudioSelectionBox` (0x14166da10) just forwards to a Studio-provided `studioSelectionCallbackGuiBase2d` (set by the Qt Studio shell via `setStudioSelectionCallbackGuiBase2d`); no callback, no box — this is edit-mode only.
6. **Property-pane visibility is a compile-out in Studio**: `isPropertyHiddenInStudio` rules found:
   - `GuiBase2d` (0x14161f080): `SelectionBehaviorUp/Down/Left/Right` hidden unless the object has a `SelectionGroup`;
   - `GuiLayerCollector`: inherits GuiBase2d's;
   - `BillboardGui` (0x141678150): Brightness conditional; `Distance*` always hidden;
   - `CanvasGroup` (0x1418982d0): `Clipping` always hidden;
   - `SurfaceGui` (0x141681c10): own rules on top of base (in `v8datamodel/SurfaceGui.c`).
7. Everything layout/step related (`stepLayouts`, `onStepped`, `onHeartbeat`, quadtree rebuild) runs through `Stepped`/`Heartbeat` in both modes — billboard projection recomputation therefore also happens continuously in edit mode.

---

## 9. FFlags spotted in these paths (behavior switches worth citing)
`UserFlagEnableNewVRSystem`, `UserFlagEnableVRUpdate2`, `VRUsePointerForTextBoxPreprocessing`, `GuiBrightness`, `ViewportFrameUseRender2dBorder2`, `ViewportFrameEnableMirroring` (DF), `RenderCanvasGroupToTexture`, `SortBillboardGuisForInput`, `FixSmallSurfaceGuiMouseInput`, `AvoidUnnecessaryQuadtreeLock`, `ReduceGuiStateGfxGuiInvalidation`, `AdaptiveUILayoutSupportsRendering`, `g_flagsLoaded/DebugTraceCreatableCounts3`.

## 10. Safety notes on the extraction
Archive contained 19,478 files / 481 dirs; **no symlinks, hardlinks, device nodes, absolute paths, or `..` traversal entries** (verified before extraction); extracted with `--no-same-owner --no-same-permissions` into `./2022-full/` (644 MB). Everything answered here came from static reads of that tree.
