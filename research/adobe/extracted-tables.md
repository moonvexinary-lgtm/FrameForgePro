
===== FILE: channel-effects.md =====
| **Invert Effect Channels and Controls** | **Description** |
| --- | --- |
| Alpha | This effect inverts the image's alpha channel. The alpha channel isn’t a color channel. It specifies transparency (inverts transparency values). |
| Blend With Original | This effect adjusts the percentage of the original image that blends with the inverted result (0–100%). For example, if you set this value to 100%, the effect has no visible result on the clip. The original image doesn't show through if you set this value to zero %. |
| Channel | This effect lets you choose which channel to invert. RGB, Red, Green, Blue, Alpha, etc. |
| HLS/Hue/Lightness/Saturation | This effect inverts all three calculated color channels. Hue, Lightness, and Saturation each invert an individual color channel. |
| RGB/Blue/Green/Red | This effect changes the color of your video. The RGB effect inverts all color channels together. The blue effect only inverts the blue channel. The green effect only inverts the green channel. The red effect only inverts the red channel. |

===== FILE: generate-effects.md =====
| **Effect Name** | **Description** |
| --- | --- |
| 4-Color Gradient | Creates a gradient with four colors. Use the Positions and Colors controls to animate the positions and colors of the four effect points, blending them into solid-color circles, each with an effect point as its center. |
| Lens Flare | Simulates what happens when bright light hits a camera lens. |
| Lightning | Creates animated lightning bolts, Jacob’s Ladders, or other electric visuals between two specified points in a clip. The effect animates automatically across the clip’s duration without keyframes. |
| Ramp | Creates a color gradient, either linear or radial. You can animate the start and end positions, vary the colors, and use Ramp Scatter control to disperse the ramp colors and eliminate banding. |

===== FILE: image-control-effects.md =====
| **Effect name** | **Description** |
| --- | --- |
| Black & White | This effect turns your clip to grayscale, and you cannot animate this effect with keyframes. Use Effects icon in the Effects panel to toggle this effect. |
| Color Pass | This effect turns a video into grayscale except for one chosen color, making the chosen color stand out. For instance, in a basketball game clip, you can keep the basketball's color while the rest is in grayscale.This effect highlights colors, not specific objects. |
| Color Replace | This effect replaces a selected color with a new color, keeping gray levels intact. You can change an object's color in an image by selecting it and adjusting the controls to create a different color. |
| Gamma Correction | This effect changes a clip's brightness by adjusting the mid tones (the middle-gray levels) without touching shadows and highlights. 10 is the default setting, but you can adjust it from 1 to 28 in the Effect Controls panel. |

===== FILE: immersive-video-effects.md =====
| **Immersive Video effect name** | **Description** |
| --- | --- |
| VR Blur | The effect blurs without detracting from the immersive experience and is specifically designed for 360° and 180° footage. Everything, including poles and edges, is subtly smoothed throughout the view. No matter where the viewer looks, it maintains a natural appearance by intelligently distributing the blur evenly throughout the scene. It functions flawlessly even on large, high-resolution VR projects because it is GPU-accelerated. |
| VR Chromatic Aberrations | The effect simulates the lens flaws you might see in a VR headset by adding a subtle color shift to the edges of your 360° video. It produces a delicate, lifelike color fringing that heightens the sense of immersion, making your footage look more stylized or cinematic. |
| VR Color Gradients | The effect lets you add smooth color transitions across your 360° footage. It creates subtle gradient fills that blend seamlessly across the spherical view, perfect for adding atmospheric tones or stylish color effects while maintaining immersion |
| VR De-Noise | The effect helps reduce unwanted grain or noise in your 360° footage, making it smoother and cleaner. It’s perfect for improving the quality of low-light scenes or footage with digital noise, without losing detail, so your immersive video looks crisp and professional. |
| VR Digital Glitch | The effect adds a cool, techy distortion to your 360° video, simulating digital glitches. It creates quick, eye-catching disruptions in the footage, perfect for adding a futuristic or edgy vibe to your VR project, all while keeping the immersive feel intact. |
| VR Fractal Noise | The effect creates dynamic, organic-looking textures, like smoke, clouds, or abstract patterns. It generates random, evolving noise across your footage, perfect for adding visual interest, backgrounds, or special effects that feel natural and unpredictable. |
| VR Glow | The effect adds a soft, radiant glow around bright areas of your 360° footage. It enhances light sources, creating a dreamy, ethereal look without disrupting the immersive feel. It is perfect for highlighting lights, adding atmosphere, or making your footage feel more magical. |
| VR Plane to Sphere | The effect takes flat 2D elements like images, text, or logos and wraps them around a 360° video sphere. It helps integrate these elements smoothly into immersive VR scenes, ensuring they look natural and undistorted, no matter how the viewer looks around. |
| VR Projection | The effect projects 2D footage onto a 360° spherical surface, allowing you to place flat images or videos within a VR environment seamlessly. It’s perfect for creating realistic backgrounds, textures, or graphics that integrate into the immersive scene. |
| VR Rotate Sphere | The effect lets you rotate your 360° footage on the X, Y, or Z axis. This helps adjust the view or align the horizon in VR videos, giving you complete control over the scene’s orientation without distorting the immersive experience. |
| VR Sharpen | The effect enhances fine details and improves the clarity of your 360° video. It's ideal for enhancing the quality of virtual reality videos, particularly in low-resolution or blurry areas, by uniformly sharpening the image throughout the spherical scene without causing distortion. |

===== FILE: noise-grain-effects.md =====
| **Noise & Grain effect name** | **Description** |
| --- | --- |
| Noise | The effect adds random pixels to your footage, creating a grainy, static-like texture. It’s great for simulating the look of older cameras, adding a gritty feel, or matching footage with different sources, giving your video a more raw, vintage, or analog vibe. |

===== FILE: perspective-effects.md =====
| **Perspective effect** | **Description** |
| --- | --- |
| Basic 3D | This effect lets you rotate and move clips in 3D space, adjusting them around horizontal and vertical axes, and bringing them closer or further away. You can also add a specular highlight, simulating light reflecting off a rotated surface. The light source for this highlight is always positioned overhead, behind, and to the viewer's left, so the image needs to be tilted backward to see the reflection. These highlights can make the 3D effect look more realistic. |
| Drop Shadow | This effect adds a shadow that appears behind the clip. The alpha channel of the clip determines the shape of the drop shadow. When you add a drop shadow to a clip, a soft-edged outline of its alpha channel appears behind it, as if a shadow is cast on the background or underlying objects. Unlike most other effects, Drop Shadow can create a shadow outside the bounds of the clip (the dimensions of the clip’s source). To render the shadow without the image, select Shadow Only. |

===== FILE: removed-effects.md =====
| **Effect name** | **Platform** | **Alternative** |
| --- | --- | --- |
| Anti-Alias | Windows | Third party |
| Arithmetic | Windows, macOS, macOS ARM | Lumetri |
| Auto Color | Windows, macOS | Lumetri |
| Auto Contrast | Windows, macOS | Lumetri |
| Auto Levels | Windows, macOS | Lumetri |
| Bevel Alpha | Windows, macOS, macOS ARM | Photoshop PSD |
| Bevel Edges | Windows, macOS, macOS ARM | Photoshop PSD |
| Blend | Windows, macOS, macOS ARM | Blend Modes |
| Calculations | Windows, macOS, macOS ARM | Lumetri and Blend Modes |
| Camera Blur | macOS | Gaussian Blur effect |
| Camera View | Windows, macOS | Photoshop, After Effects |
| Cell Pattern | Windows, macOS, macOS ARM | Photoshop, After Effects |
| Change Color | Windows, macOS | Lumetri |
| Change-to-Color | Windows, macOS | Lumetri |
| Channel Mixer | Windows, macOS | Lumetri |
| ChannelBlur | Windows, macOS, macOS ARM | Blurs |
| Checkboard | Windows, macOS, macOS ARM | Photoshop PSD |
| Circle | Windows, macOS, macOS ARM | Masking Tools |
| Clip Name | Windows, macOS, macOS ARM | Metadata Burn-in |
| Color Balance | Windows, macOS | Lumetri |
| Compound Blur | Windows, macOS, macOS ARM | GPU-native blurs |
| CompoundArithmetic | Windows, macOS, macOS ARM | Blend Modes |
| Convolution Kernel | Windows, macOS, macOS ARM | Lumetri |
| Difference Matte | Windows, macOS, ARM macOS | Blend Mode |
| Dust & Scratches | Windows, macOS, macOS ARM | Gaussian Blur |
| Ellipse | Windows, macOS, macOS ARM | Masking |
| Emboss | Windows, macOS, macOS ARM | Color Emboss |
| Equalize | Windows, macOS | Lumetri |
| Eyedropper Fill | Windows, macOS, macOS ARM | Lumetri |
| Fast Blur | Windows, macOS, macOS ARM | Gaussian Blur |
| Fast Color Corrector | Windows, macOS | Lumetri |
| Ghosting | Windows | Opacity, Speed Effects |
| Grid | Windows, macOS, macOS ARM | Photoshop, Illustrator |
| Horizontal Hold | Windows | Third party |
| Image Matte Key | Windows, macOS, macOS ARM | Blend Modes |
| Leave Color | Windows, macOS | Lumetri |
| Luma Corrector | Windows, macOS | Lumetri |
| Luma Curves | Windows, macOS | Lumetri |
| Median (Legacy) | Windows, macOS, macOS ARM | Lumetri, VR-Denoise |
| Noise Alpha | Windows, macOS, macOS ARM | Noise |
| Noise HLS | Windows, macOS, macOS ARM | Noise |
| Noise HLS Auto | Windows, macOS, macOS ARM | Noise |
| Non Red Key | Windows, macOS, macOS ARM | Ultra Key |
| Paint Bucket | Windows, macOS, macOS ARM | Lumetri |
| Radial Shadow | Windows, macOS, macOS ARM | Third party |
| Remove Matte | Windows, macOS, macOS ARM | Keyers and Blend Modes |
| RGB Color Corrector | Windows, macOS Intel | Lumetri |
| RGB Curves | Windows, macOS Intel | Lumetri |
| Set Matte | Windows, macOS, macOS ARM | Keyers and Blend Modes |
| Shadow/ Highlight | Windows, macOS | Lumetri |
| Solarize | Windows, macOS, macOS ARM | Third party |
| Solid Composite | Windows, macOS, macOS ARM | Generators |
| Video Limiter (Legacy) | Windows, macOS | Video Limiter |
| Write-On | Windows, macOS, macOS ARM | Essential Graphics, After Effects |
| Texturize | Windows, macOS, macOS ARM | Third party |
| Three-Way Color Corrector | Windows, macOS | Lumetri |
| Threshold | Windows, macOS, macOS ARM | Lumetri, Color Tools |
| Timecode | Windows, macOS, macOS ARM | Metadata Burn-in |
| Vertical Hold | Windows | Third party |
| **Transition Name** | **Platform** | **Alternative** |
| --- | --- | --- |
| Band Slide | macOS | Third party |
| Band Wipe | macOS | Third party |
| Checker Board | macOS | Third party |
| Checker Wipe | macOS | Third party |
| Clock Wipe | macOS | Third party |
| Cube Spin | Windows, macOS, macOS ARM | Third party |
| Flip Over | Windows, macOS, macOS ARM | Third party |
| Gradient Wipe | Windows, macOS, macOS ARM | Third party |
| Paint Splatter | macOS | Third party |
| Pin Wheel | macOS | Third party |
| Radial Wipe | macOS, macOS ARM | Third party |
| Random Blocks | macOS | Third party |
| Random Wipe | macOS | Third party |
| Spiral Boxes | macOS | Third party |
| Venetian Blinds | macOS, macOS ARM | Third party |
| Wedge Wipe | macOS | Third party |
| Zig-Zag Blocks | macOS | Third party |

===== FILE: video-dissolve-transitions.md =====
| **Dissolve transition type** | **Description** |
| --- | --- |
| Additive Dissolve | Adds color information from clip B to clip A, and then subtracts the color information of clip A from clip B. |
| Cross Dissolve | Fades out clip A while fading in clip B. Works well at the beginning or end of a clip when you want to fade in or out from black. |
| Dip To Black | Note Fades clip A to black and then fades from black to clip B. Using dip to black at the beginning or end of a clip will also affect a video on a lower track, something not always expected when a simple fade in/out of the targeted clip is wanted. The cross-dissolve transition may work better for this. |
| Dip To White | Fades clip A to white, then fades from white to clip B. |
| Film Dissolve | Blends more realistically, and dissolves look the way they should. Blends in a linear color space (where gamma equals 1.0). |
| Morph Cut | Creates a harmonious transition between two similar clips, usually of a person speaking. It analyses the frames and uses face tracking and interpolation to smooth over jump cuts, making it look like a continuous shot. |
| Non-Additive Dissolve | Blends two clips by crossfading their pixel values without brightening the result, unlike standard dissolves that can create a glow or lighten overlapping areas. It makes a more neutral, natural-looking fade between clips. |

===== FILE: video-transitions.md =====
| **Transition c****ategory** | **Available transitions** |
| --- | --- |
| Dissolve | * Additive Dissolve * Cross Dissolve * Dip To Black * Dip To White * Film Dissolve * Morph Cut * Non-Additive Dissolve |
| Immersive Video | * VR Chroma Leaks * VR Gradient Wipe * VR Iris Wipe * VR Light Leaks * VR Light Rays * VR Mobius Zoom * VR Random Blocks * VR Spherical Blur |
| Iris | * Iris Box * Iris Cross * Iris Diamond * Iris Round |
| Page Peel | * Page Peel * Page Turn |
| Slide | * Band Slide * Center Split * Push * Slide * Split * Whip |
| Wipe | * Band Wipe * Barn Doors * Checker Wipe * CheckerBoard * Clock Wipe * Inset * Paint Splatter * Pinwheel * Radial Wipe * Random Blocks * Random Wipe * Spiral Boxes * Venetian Blinds * Wedge Wipe * Wipe * Zig-Zag Blocks |
| Zoom | * Cross Zoom |

===== FILE: warp-stabilizer-settings.md =====
| **UI option** | **Function** |
| --- | --- |
| Result | Control the intended result for the footage (Smooth Motion or No Motion). * Smooth Motion (default)**:**Retains the original camera movement but makes it smoother. When selected, Smoothness is enabled to control how smooth the camera movement becomes. * No Motion**:**Attempts to remove all camera motion from the shot. When selected, the Crop Less <-> Smooth More function is disabled in the Advanced section. This setting is used for footage where at least a portion of the main subject remains within the frame for the entire range being analyzed. |
| Smoothness | Choose how much the camera’s original motion is stabilized. Lower values are closer to the camera’s original motion, while higher values are smoother. Values above 100 require more cropping of the image. Enabled when the Result is set to Smooth Motion. |
| Method | Specify the most complex operation the Warp Stabilizer performs on the footage to stabilize it: * Position**:**Stabilization is based on position data only and is the most basic way footage can be stabilized. * Position, Scale, Rotation**:**Stabilization is based upon position, scale, and rotation data. If there are not enough areas to track, Warp Stabilizer chooses the previous type (Position). * Perspective**:**Uses a type of stabilization in which the entire frame is effectively corner-pinned. If there are not enough areas to track, Warp Stabilizer chooses the previous type (Position, Scale, Rotation). * Subspace Warp (default)**:**Attempts to warp various parts of the frame differently to stabilize the entire frame. If there are not enough areas to track, Warp Stabilizer chooses the previous type (Perspective). The method in use on any given frame can change across the course of the clip based on the tracking accuracy. |
| Preserve Scale | Prevent changes to the clip’s scale when selected. |
| **UI option** | **Function** |
| --- | --- |
| Framing | Control how the edges appear in a stabilizing result. Framing can be set to one of the following: * Stabilize Only: Displays the entire frame, including the moving edges. Stabilize Only shows how much work is being done to stabilize the image. Using Stabilize Only allows you to crop the footage using other methods. When selected, the Auto-scale section and Crop Less <-> Smooth More property are disabled. * Stabilize, Crop: Crops the moving edges without scaling. Stabilize, Crop is identical to using Stabilize, Crop, Auto-scale, and setting Maximum Scale to 100%. With this option enabled, the Auto-scale section is disabled, but the Crop Less <-> Smooth More property is enabled. * Stabilize, Crop, Auto-scale (default): Crops the moving edges and scales up the image to refill the frame. The automatic scaling is controlled by various properties in the Auto-scale section. * Stabilize, Synthesize Edges: Fills in the blank space created by the moving edges with content from frames earlier and later in time (controlled by Synthesis Input Range in the Advanced section). With this option, the Auto-scale section and Crop Less <-> Smooth More are disabled. Note **Note:** It is possible for artifacts to appear when there is movement at the edge of the frame, not related to camera movement |
| Auto-scale | Display the current auto-scale amount and allows you to set limits on the amount of auto-scaling. Enable Auto-scale by setting framing to Stabilize, Crop, Auto-scale. * Maximum Scale: Limits the maximum amount a clip is scaled up for stabilization. * Action-Safe Margin: When non-zero, specifies a border around the edge of the image that you don’t expect to be visible. Thus, Auto-scale does not try to fill it. |
| Additional Scale | Scale up the clip with the same result as scaling using the Scale property under Transform but avoids an extra resampling of the image. |
| **UI option** | **Function** |
| --- | --- |
| Detailed Analysis | Make the next analysis phase do extra work to find elements to track (when set to on). The resulting data (stored in the project as part of the effect) is much larger and slower with this option enabled. |
| Fast Analysis | Improve the time Premiere takes to analyze shaky footage while leaving the actual stabilization process unchanged. This option is selected by default. You can deselect the checkbox if you want. |
| Rolling Shutter Ripple | Automatically remove the rippling associated with stabilized rolling shutter footage. Automatic Reduction is the default. Select Enhanced Reduction if the footage contains larger ripples. To select either method, set the Method to Subspace Warp or Perspective. |
| Crop Less <-> Smooth More | Control the trade-off between smoothness and scaling of the cropping rectangle as it moves over the stabilized image. Lower values are smooth; however, more of the image is viewed. At 100%, the result is the same as the Stabilize Only option with manual cropping. |
| Synthesis Input Range (seconds) | Control how far backward and forward in time the synthesis process goes to fill in any missing pixels. It is used by Stabilize, Synthesize Edges framing. |
| Synthesis Edge Feather | Select the amount of feather for the synthesized pieces. It is enabled only when using the Stabilize, Synthesize Edges framing. Use the feather control to smooth over edges where the synthesized pixels join up with the original frame. |
| Synthesis Edge Cropping | Trim off the edges of each frame before it is used to combine with other frames when using the Stabilize, Synthesize Edges framing option. Use the cropping controls to crop off bad edges that are common in analog video capture or low-quality optics. By default, all edges are set to zero pixels. |
| Hide Warning Banner | Use it when you don’t want to reanalyze footage, even though there is a warning banner indicating that it must be reanalyzed. |
