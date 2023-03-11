#version 330 core

layout (location = 0) in vec2 aPos;
layout (location = 1) in vec4 inColour;
uniform ivec2 screenSize;
uniform float scale;
uniform float yCenter;

out vec4 colour;

void main()
{
	vec2 screenPos = aPos;
	screenPos.y -= yCenter;
	screenPos /= screenSize;
	screenPos *= 2 * scale;
	screenPos.x -= 1.0;
	screenPos.y *= -1.0;
	gl_Position = vec4(screenPos, 0.0, 1.0);
  colour = inColour;
}