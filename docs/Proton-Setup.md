# Using lsfg-vk with Steam Proton

Steam Proton games run inside a pressure-vessel sandbox that isolates the game's view of the filesystem. This means the Vulkan implicit layer installed at `/etc/vulkan/implicit_layer.d/` and the shared library at `/usr/lib/liblsfg-vk.so` are **not directly accessible** to the game process.

This guide covers how to make lsfg-vk work inside the Proton container.

## Quick Setup (Steam Launch Option)

1. **Ensure the layer is installed system-wide** (via pacman, the tarball release, or `fix-layer.sh`).

2. **Copy the layer .so into Proton's lib directory** so the container can load it:
   ```bash
   PROTON_LIB="$HOME/.local/share/Steam/steamapps/common/Proton - Experimental/files/lib/x86_64-linux-gnu"
   cp /usr/lib/liblsfg-vk.so "$PROTON_LIB/liblsfg-vk.so"
   ```
   > Repeat this if you update Proton (e.g. Proton Experimental gets an update).

3. **Create a layer JSON with absolute library_path** in the user's Vulkan layer directory:
   ```bash
   mkdir -p ~/.local/share/vulkan/implicit_layer.d/
   PROTON_LIB="$HOME/.local/share/Steam/steamapps/common/Proton - Experimental/files/lib/x86_64-linux-gnu"
   cat > ~/.local/share/vulkan/implicit_layer.d/VkLayer_LS_frame_generation.json << EOF
   {
     "file_format_version": "1.1.0",
     "layer": {
       "name": "VK_LAYER_LS_frame_generation",
       "description": "Lossless Scaling frame generation layer",
       "implementation_version": "2",
       "library_path": "$PROTON_LIB/liblsfg-vk.so",
       "type": "GLOBAL",
       "api_version": "1.4.328",
       "disable_environment": {
         "DISABLE_LSFG": "1"
       }
     }
   }
   EOF
   ```
   > The absolute `library_path` is critical. The default JSON uses a relative path (`liblsfg-vk.so`) which the container's Vulkan loader cannot resolve because its `/usr/lib/` is different from the host's.

4. **Set the Steam launch option** for your game (right-click game > Properties > General > Launch Options):
   ```
   VK_INSTANCE_LAYERS=VK_LAYER_LS_frame_generation %command%
   ```

5. **Launch the game.** You should see frame generation active. Verify by checking:
   - The lsfg-vk GUI's status indicator
   - `cat /tmp/lsfg-vk-metrics.json` (if metrics are enabled)
   - Steam's FPS counter should show a higher framerate

## Why This Is Necessary

Proton's `pressure-vessel` container:
- Overrides `VK_IMPLICIT_LAYER_PATH` to point inside the container
- Overrides `VK_LAYER_PATH` to point inside the container
- Mounts the host filesystem at `/run/host/` instead of at `/`
- The game's `LD_LIBRARY_PATH` includes Proton's lib dirs and pressure-vessel overrides, but **not** `/usr/lib/`

The host's `~/.local/share/vulkan/implicit_layer.d/` is one of the few directories that IS visible inside the container, which is why we place our layer JSON there.

## Updating After Changes

If you update any of the following, re-run the relevant steps:
- **lsfg-vk updated**: Re-copy the .so (step 2)
- **Proton updated**: Re-copy the .so (step 2) and update the JSON path (step 3)
- **Changed GPU**: Update the `gpu` field in your profile config

## Using GE-Proton

If you use GE-Proton instead of Proton Experimental, adjust the path:
```bash
PROTON_LIB="$HOME/.local/share/Steam/compatibilitytools.d/GE-Proton10-34/files/lib/x86_64-linux-gnu"
```
Replace `GE-Proton10-34` with your installed version.

## Native Linux Games

Native Linux games (not running through Proton) do **not** need any of these workarounds. The implicit layer loads automatically. Just configure a profile with the game's executable name and start the game.

## Verifying the Layer Is Loaded

To confirm the layer is loaded inside a running game:
```bash
# Check if the .so is mapped in the game process
GAME_PID=$(pgrep -f 'YourGame.exe' | head -1)
cat /proc/$GAME_PID/maps | grep lsfg
```

If there is output, the layer is loaded. If empty, the layer failed to load.

You can also enable Vulkan loader debug logging by adding to the Steam launch option:
```
VK_LOADER_DEBUG=layer VK_INSTANCE_LAYERS=VK_LAYER_LS_frame_generation %command%
```
Then check `~/.steam/steam/logs/console-linux.txt` for messages about `VK_LAYER_LS_frame_generation`.
