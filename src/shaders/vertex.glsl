#version 330 core
layout (location = 0) in vec2 aPos;
layout (location = 1) in vec2 aTexCoord;
uniform ivec2 screenSize;
uniform float scale;

out vec2 TexCoord;

void main()
{
	vec2 screenPos = aPos / screenSize;
	screenPos *= scale;
	screenPos.x -= 1.0;
	screenPos.y *= -1.0;
	gl_Position = vec4(screenPos, 0.0, 1.0);
	TexCoord = vec2(aTexCoord.x, aTexCoord.y);
}